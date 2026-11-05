RetroOS bootloader settings for the Raspberry Pi 5
==================================================

This card writes Raspberry Pi's 2026-10-30 bootloader to the Pi 5's EEPROM
with settings for a faster RetroOS start:

  BOOT_ORDER=0xf41             SD card first, then USB, then try again
  BOOT_UART=0                  no debug output on the serial port
  NET_INSTALL_AT_POWER_ON=0    no network-install screen after a cold boot
  NET_INSTALL_ENABLED=0        Shift at power-on no longer starts a network
                               install (Space still opens the boot menu)

To use it:

1. Flash retroos-bootloader-rpi5.img to a spare SD card
   (Raspberry Pi Imager -> Choose OS -> Use custom).
2. Power off the Pi, insert this card, power on.
3. Wait at least 10 seconds. Success: the screen turns green and the green
   LED blinks rapidly. Red or a different blink pattern: the update failed;
   the old bootloader is still there.
4. Power off, put the RetroOS card back, power on.

To go back to Raspberry Pi's defaults: Raspberry Pi Imager -> Choose OS ->
Misc utility images -> Bootloader (Pi 5 family) -> SD Card Boot.
