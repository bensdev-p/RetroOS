//! retro-kms: the smallest possible way to put pixels on the screen and read
//! keys before any compositor is running (the POST screen, the Startup Menu).
//!
//! Display: legacy KMS modeset with one XRGB8888 dumb buffer on the first
//! connected connector of the first card that has one (on a Pi 4/5 that is
//! the vc4 display device; v3d is render-only and is skipped naturally).
//! Input: raw evdev key events from every `/dev/input/event*` device.

use drm::buffer::{Buffer as _, DrmFourcc};
use drm::control::{
    connector, crtc, dumbbuffer::DumbBuffer, framebuffer, Device as ControlDevice, Mode,
};
use drm::Device;
use retro_gfx::Canvas;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl Device for Card {}
impl ControlDevice for Card {}

pub struct Display {
    card: Card,
    db: DumbBuffer,
    fb: framebuffer::Handle,
    crtc: crtc::Handle,
    conn: connector::Handle,
    mode: Mode,
    pub width: u32,
    pub height: u32,
    pitch: u32,
    pub connector_name: String,
    pub path: String,
    vt: Option<File>,
}

const KDSETMODE: libc::c_ulong = 0x4B3A;
const KD_TEXT: libc::c_int = 0;
const KD_GRAPHICS: libc::c_int = 1;

impl Display {
    /// Open the first card with a connected display and light it up (black).
    pub fn open() -> io::Result<Display> {
        let mut last_err = io::Error::new(io::ErrorKind::NotFound, "no display connected");
        for i in 0..8 {
            let path = format!("/dev/dri/card{i}");
            match Self::open_card(&path) {
                Ok(d) => return Ok(d),
                Err(e)
                    if e.kind() == io::ErrorKind::NotFound
                        && !std::path::Path::new(&path).exists() => {}
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    fn open_card(path: &str) -> io::Result<Display> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(path)?;
        let card = Card(f);
        let res = card.resource_handles()?;
        let conns: Vec<connector::Info> = res
            .connectors()
            .iter()
            .filter_map(|c| card.get_connector(*c, true).ok())
            .collect();
        let con = conns
            .iter()
            .find(|c| c.state() == connector::State::Connected && !c.modes().is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no connected connector"))?;
        // Preferred mode first, else the first listed.
        let mode = con
            .modes()
            .iter()
            .find(|m| {
                m.mode_type()
                    .contains(drm::control::ModeTypeFlags::PREFERRED)
            })
            .copied()
            .unwrap_or(con.modes()[0]);
        let crtc = con
            .current_encoder()
            .and_then(|e| card.get_encoder(e).ok())
            .and_then(|e| e.crtc())
            .or_else(|| {
                // Any CRTC usable by any of the connector's encoders.
                con.encoders()
                    .iter()
                    .filter_map(|e| card.get_encoder(*e).ok())
                    .find_map(|enc| res.filter_crtcs(enc.possible_crtcs()).first().copied())
            })
            .ok_or_else(|| io::Error::other("no usable CRTC"))?;
        let (w, h) = mode.size();
        let mut db = card.create_dumb_buffer((w as u32, h as u32), DrmFourcc::Xrgb8888, 32)?;
        let pitch = db.pitch();
        {
            let mut map = card.map_dumb_buffer(&mut db)?;
            map.as_mut().fill(0);
        }
        let fb = card.add_framebuffer(&db, 24, 32)?;
        card.set_crtc(crtc, Some(fb), (0, 0), &[con.handle()], Some(mode))?;
        let name = format!("{:?}-{}", con.interface(), con.interface_id());
        let vt = graphics_vt();
        Ok(Display {
            card,
            db,
            fb,
            crtc,
            conn: con.handle(),
            mode,
            width: w as u32,
            height: h as u32,
            pitch,
            connector_name: name,
            path: path.to_string(),
            vt,
        })
    }

    /// Copy a full-screen canvas to the scan-out buffer.
    pub fn present(&mut self, c: &Canvas) -> io::Result<()> {
        let (w, h, pitch) = (
            self.width.min(c.width) as usize,
            self.height.min(c.height) as usize,
            self.pitch as usize,
        );
        let mut map = self.card.map_dumb_buffer(&mut self.db)?;
        let dst = map.as_mut();
        for y in 0..h {
            let src = &c.px[y * c.width as usize..y * c.width as usize + w];
            let row = &mut dst[y * pitch..y * pitch + w * 4];
            for (d, s) in row.chunks_exact_mut(4).zip(src) {
                d.copy_from_slice(&s.to_le_bytes());
            }
        }
        Ok(())
    }

    /// Re-apply our mode (e.g. after another client changed it).
    pub fn restore(&self) -> io::Result<()> {
        self.card.set_crtc(
            self.crtc,
            Some(self.fb),
            (0, 0),
            &[self.conn],
            Some(self.mode),
        )
    }

    /// Give up DRM master so another program can take the display.
    pub fn release(&self) {
        let _ = self.card.release_master_lock();
    }

    pub fn refresh_hz(&self) -> u32 {
        self.mode.vrefresh()
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        let _ = self.card.destroy_framebuffer(self.fb);
        if let Some(vt) = &self.vt {
            unsafe {
                libc::ioctl(vt.as_raw_fd(), KDSETMODE, KD_TEXT);
            }
        }
    }
}

/// Put the active VT into graphics mode so the kernel console doesn't draw over us.
fn graphics_vt() -> Option<File> {
    for p in ["/dev/tty0", "/dev/tty1"] {
        if let Ok(f) = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(p)
        {
            if unsafe { libc::ioctl(f.as_raw_fd(), KDSETMODE, KD_GRAPHICS) } == 0 {
                return Some(f);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Keys

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Esc,
    Enter,
    Up,
    Down,
    Delete,
    F(u8),
    Digit(u8),
    Space,
    Other(u16),
}

fn decode(code: u16) -> Key {
    match code {
        1 => Key::Esc,
        28 | 96 => Key::Enter,
        103 => Key::Up,
        108 => Key::Down,
        111 | 83 => Key::Delete, // Delete, keypad '.'/Del
        57 => Key::Space,
        2..=10 => Key::Digit((code - 1) as u8),
        11 => Key::Digit(0),
        59..=68 => Key::F((code - 58) as u8),
        87 => Key::F(11),
        88 => Key::F(12),
        // Keypad digits 1-9 (KP1=79, KP2=80, KP3=81, KP4=75, KP5=76, KP6=77, KP7=71, KP8=72, KP9=73).
        79 => Key::Digit(1),
        80 => Key::Digit(2),
        81 => Key::Digit(3),
        75 => Key::Digit(4),
        76 => Key::Digit(5),
        77 => Key::Digit(6),
        71 => Key::Digit(7),
        72 => Key::Digit(8),
        73 => Key::Digit(9),
        c => Key::Other(c),
    }
}

/// All evdev devices, opened non-blocking. Devices appearing later are picked
/// up by [`Keyboard::rescan`].
pub struct Keyboard {
    devs: Vec<(String, File)>,
}

impl Keyboard {
    pub fn open() -> Keyboard {
        let mut k = Keyboard { devs: Vec::new() };
        k.rescan();
        k
    }

    pub fn rescan(&mut self) {
        let Ok(rd) = std::fs::read_dir("/dev/input") else {
            return;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("event") || self.devs.iter().any(|(n, _)| *n == name) {
                continue;
            }
            if let Ok(f) = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(e.path())
            {
                self.devs.push((name, f));
            }
        }
    }

    pub fn count(&self) -> usize {
        self.devs.len()
    }

    /// Wait up to `timeout` for a key press (or auto-repeat). `None` on timeout.
    pub fn next(&mut self, timeout: Duration) -> Option<Key> {
        let mut fds: Vec<libc::pollfd> = self
            .devs
            .iter()
            .map(|(_, f)| libc::pollfd {
                fd: f.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        if fds.is_empty() {
            std::thread::sleep(timeout);
            return None;
        }
        let n = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                timeout.as_millis() as i32,
            )
        };
        if n <= 0 {
            return None;
        }
        let mut result = None;
        for (i, p) in fds.iter().enumerate() {
            if p.revents & libc::POLLIN == 0 {
                continue;
            }
            let fd = self.devs[i].1.as_raw_fd();
            loop {
                let mut ev: libc::input_event = unsafe { std::mem::zeroed() };
                let sz = std::mem::size_of::<libc::input_event>();
                let r = unsafe { libc::read(fd, (&mut ev as *mut libc::input_event).cast(), sz) };
                if r != sz as isize {
                    break;
                }
                // EV_KEY press (1) or repeat (2); BTN_* codes (>= 0x100) are mouse buttons.
                if ev.type_ == 1
                    && (ev.value == 1 || ev.value == 2)
                    && ev.code < 0x100
                    && result.is_none()
                {
                    result = Some(decode(ev.code));
                }
            }
        }
        result
    }

    /// Drop any queued events.
    pub fn drain(&mut self) {
        while self.next(Duration::from_millis(0)).is_some() {}
    }
}
