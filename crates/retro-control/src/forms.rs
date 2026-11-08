//! Small forms behind action buttons (Control Panel → Users): a few typed
//! fields and the administrator's own password, described here once and
//! drawn by both front-ends (D-046).

use crate::{Page, Value};
use retro_sys::users;

pub const ACTION_ADD_USER: &str = "add-user";
pub const ACTION_PASSWORD: &str = "password";
pub const ACTION_ADMIN: &str = "admin";
pub const ACTION_REMOVE_USER: &str = "remove-user";

/// The button's own text.
pub fn action_label(action: &str) -> &'static str {
    match action {
        ACTION_ADD_USER => "Add...",
        ACTION_PASSWORD | ACTION_ADMIN => "Change...",
        ACTION_REMOVE_USER => "Remove...",
        _ => "Open...",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormField {
    pub key: &'static str,
    pub label: &'static str,
    /// Typed as `*`s.
    pub secret: bool,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub action: &'static str,
    pub title: String,
    /// Lines above the fields.
    pub intro: Vec<String>,
    pub fields: Vec<FormField>,
    /// The confirming button ("Add", "Remove", …).
    pub button: &'static str,
    /// The user the form is about ("" for a new one).
    pub target: String,
}

impl Form {
    pub fn value(&self, key: &str) -> String {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.clone())
            .unwrap_or_default()
    }
}

fn field(key: &'static str, label: &'static str, secret: bool) -> FormField {
    FormField {
        key,
        label,
        secret,
        value: String::new(),
    }
}

/// Every form ends with this: who is allowed to do it is checked by sudo.
fn your_password() -> FormField {
    field("auth", "Your password", true)
}

/// The form behind `action` on `page`; Err when it cannot open (no user
/// selected, …).
pub fn form_for(action: &str, page: &Page) -> Result<Form, String> {
    let all = users::list();
    let sel = page
        .field("users")
        .map(|f| f.text())
        .filter(|s| !s.is_empty());
    let user = |verb: &str| -> Result<users::User, String> {
        let name = sel
            .clone()
            .ok_or(format!("Choose the user to {verb} first."))?;
        all.iter()
            .find(|u| u.name == name)
            .cloned()
            .ok_or(format!("There is no user called {name}."))
    };
    Ok(match action {
        ACTION_ADD_USER => Form {
            action: ACTION_ADD_USER,
            title: "New User".into(),
            intro: vec!["A new standard user (not an administrator).".into()],
            fields: vec![
                field("name", "User name", false),
                field("full", "Full name", false),
                field("pw", "Password", true),
                field("again", "Type it again", true),
                your_password(),
            ],
            button: "Add",
            target: String::new(),
        },
        ACTION_PASSWORD => {
            let u = user("change the password of")?;
            Form {
                action: ACTION_PASSWORD,
                title: "Change Password".into(),
                intro: vec![format!("A new password for {}.", u.label())],
                fields: vec![
                    field("pw", "New password", true),
                    field("again", "Type it again", true),
                    your_password(),
                ],
                button: "Change",
                target: u.name,
            }
        }
        ACTION_ADMIN => {
            let u = user("change")?;
            let intro = if u.admin {
                vec![
                    format!("Take away {}'s administrator rights?", u.label()),
                    "They will no longer be able to change settings".into(),
                    "for everyone or install programs.".into(),
                ]
            } else {
                vec![
                    format!("Make {} an administrator?", u.label()),
                    "They will be able to change settings for".into(),
                    "everyone and install programs.".into(),
                ]
            };
            Form {
                action: ACTION_ADMIN,
                title: "Administrator Rights".into(),
                intro,
                fields: vec![your_password()],
                button: if u.admin { "Take Away" } else { "Make Admin" },
                target: u.name,
            }
        }
        ACTION_REMOVE_USER => {
            let u = user("remove")?;
            if u.name == users::me() {
                return Err("You cannot remove your own account.".into());
            }
            Form {
                action: ACTION_REMOVE_USER,
                title: "Remove User".into(),
                intro: vec![
                    format!("Remove {}?", u.label()),
                    format!("Their files stay in /home/{}.", u.name),
                ],
                fields: vec![your_password()],
                button: "Remove",
                target: u.name,
            }
        }
        _ => return Err("Unknown action.".into()),
    })
}

/// Check the form before asking the system (a message for the user).
pub fn check(form: &Form) -> Result<(), String> {
    if form.action == ACTION_ADD_USER {
        users::check_new_name(form.value("name").trim(), &users::list())?;
    }
    if matches!(form.action, ACTION_ADD_USER | ACTION_PASSWORD) {
        if let Some(p) = users::password_problem(&form.value("pw"), &form.value("again")) {
            return Err(p.into());
        }
    }
    if form.value("auth").is_empty() {
        return Err("Type your own password to confirm.".into());
    }
    Ok(())
}

/// Do what the form says; Ok carries what to tell the user.
pub fn run(form: &Form) -> Result<String, String> {
    check(form)?;
    let auth = form.value("auth");
    let t = &form.target;
    match form.action {
        ACTION_ADD_USER => {
            let name = form.value("name").trim().to_string();
            users::add(&name, form.value("full").trim(), &form.value("pw"), &auth)?;
            Ok(format!("{name} can now log on."))
        }
        ACTION_PASSWORD => {
            users::set_password(t, &form.value("pw"), &auth)?;
            Ok(format!("{t}'s password has been changed."))
        }
        ACTION_ADMIN => {
            let now = users::list().iter().any(|u| u.name == *t && u.admin);
            users::set_admin(t, !now, &auth)?;
            Ok(if now {
                format!("{t} is now a standard user.")
            } else {
                format!("{t} is now an administrator. It applies the next time they log on.")
            })
        }
        ACTION_REMOVE_USER => {
            users::remove(t, &auth)?;
            Ok(format!("{t} has been removed."))
        }
        _ => Err("Unknown action.".into()),
    }
}

/// Is `v` an action that opens a form (rather than one the front-end knows)?
pub fn is_form_action(v: &Value) -> bool {
    matches!(
        v,
        Value::Action(ACTION_ADD_USER | ACTION_PASSWORD | ACTION_ADMIN | ACTION_REMOVE_USER)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_new_user_form_checks_before_running() {
        let page = crate::load(crate::Applet::Users);
        let mut f = form_for(ACTION_ADD_USER, &page).unwrap();
        assert_eq!(f.fields.last().unwrap().key, "auth");
        assert!(check(&f).is_err(), "empty");
        let set = |f: &mut Form, k: &str, v: &str| {
            f.fields.iter_mut().find(|x| x.key == k).unwrap().value = v.into();
        };
        set(&mut f, "name", "sam_test_9");
        set(&mut f, "pw", "one");
        set(&mut f, "again", "two");
        assert_eq!(
            check(&f).unwrap_err(),
            "The two passwords are not the same."
        );
        set(&mut f, "again", "one");
        assert_eq!(check(&f).unwrap_err(), "Type your own password to confirm.");
        set(&mut f, "auth", "x");
        assert!(check(&f).is_ok());
        assert_eq!(action_label(ACTION_REMOVE_USER), "Remove...");
    }
}
