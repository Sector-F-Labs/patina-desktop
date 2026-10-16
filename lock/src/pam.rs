//! Checking a password through PAM, as swaylock and login screens do. The
//! few calls needed are declared here; Linux-PAM and OpenPAM (FreeBSD)
//! share them.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

/// The PAM service: /etc/pam.d/patina-lock, which `make install-session`
/// installs. Without it, Linux-PAM and OpenPAM use `other`.
const SERVICE: &CStr = c"patina-lock";

const PAM_SUCCESS: c_int = 0;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_BUF_ERR: c_int = 5;
// The error codes past these differ: OpenPAM keeps the XSSO numbering.
#[cfg(target_os = "linux")]
const PAM_CONV_ERR: c_int = 19;
#[cfg(target_os = "linux")]
const PAM_AUTH_ERR: c_int = 7;
#[cfg(not(target_os = "linux"))]
const PAM_CONV_ERR: c_int = 6;
#[cfg(not(target_os = "linux"))]
const PAM_AUTH_ERR: c_int = 9;

#[repr(C)]
struct Message {
    style: c_int,
    text: *const c_char,
}

#[repr(C)]
struct Response {
    text: *mut c_char,
    code: c_int,
}

type Converse = unsafe extern "C" fn(
    count: c_int,
    messages: *mut *const Message,
    responses: *mut *mut Response,
    data: *mut c_void,
) -> c_int;

#[repr(C)]
struct Conversation {
    converse: Converse,
    data: *mut c_void,
}

#[repr(C)]
struct Handle {
    _private: [u8; 0],
}

// Ubuntu ships libpam.so.0 without its development symlink, so it's named
// in full there.
#[cfg_attr(
    target_os = "linux",
    link(name = "libpam.so.0", kind = "dylib", modifiers = "+verbatim")
)]
#[cfg_attr(not(target_os = "linux"), link(name = "pam"))]
unsafe extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conversation: *const Conversation,
        handle: *mut *mut Handle,
    ) -> c_int;
    fn pam_authenticate(handle: *mut Handle, flags: c_int) -> c_int;
    fn pam_strerror(handle: *mut Handle, code: c_int) -> *const c_char;
    fn pam_end(handle: *mut Handle, status: c_int) -> c_int;
}

/// Whether `password` is `user`'s. Blocks, for seconds after a wrong one
/// (PAM's delay), so call it off the UI thread. `Err` is PAM's reason when
/// it's something other than a wrong password.
pub fn authenticate(user: &str, password: &str) -> Result<bool, String> {
    let user = CString::new(user).map_err(|_| "bad user name".to_string())?;
    let password = CString::new(password).map_err(|_| "bad password".to_string())?;
    let conversation = Conversation {
        converse,
        data: password.as_ptr() as *mut c_void,
    };
    let mut handle = ptr::null_mut();
    // SAFETY: the strings and the conversation outlive the handle, which
    // pam_end frees.
    unsafe {
        let code = pam_start(SERVICE.as_ptr(), user.as_ptr(), &conversation, &mut handle);
        if code != PAM_SUCCESS {
            return Err(format!("PAM didn't start (code {code})"));
        }
        let code = pam_authenticate(handle, 0);
        let result = match code {
            PAM_SUCCESS => Ok(true),
            // Anything else is a problem worth showing (no PAM config,
            // an unknown user, ...).
            PAM_AUTH_ERR => Ok(false),
            _ => Err(reason(handle, code)),
        };
        pam_end(handle, code);
        result
    }
}

unsafe fn reason(handle: *mut Handle, code: c_int) -> String {
    // SAFETY: pam_strerror returns a static string, or null.
    let text = unsafe { pam_strerror(handle, code) };
    if text.is_null() {
        format!("PAM error {code}")
    } else {
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    }
}

/// Answers PAM's prompts with the password; other messages (notes,
/// errors) get no answer. PAM frees the answers.
unsafe extern "C" fn converse(
    count: c_int,
    messages: *mut *const Message,
    responses: *mut *mut Response,
    data: *mut c_void,
) -> c_int {
    let Ok(n) = usize::try_from(count) else {
        return PAM_CONV_ERR;
    };
    if n == 0 || messages.is_null() || responses.is_null() {
        return PAM_CONV_ERR;
    }
    // SAFETY: PAM passes `count` messages, as an array of pointers (the
    // layout both Linux-PAM and OpenPAM use), and frees what's allocated
    // here with free(3).
    unsafe {
        let answers = libc::calloc(n, size_of::<Response>()) as *mut Response;
        if answers.is_null() {
            return PAM_BUF_ERR;
        }
        for i in 0..n {
            let message = *messages.add(i);
            if message.is_null() {
                continue;
            }
            if matches!((*message).style, PAM_PROMPT_ECHO_OFF | PAM_PROMPT_ECHO_ON) {
                let text = libc::strdup(data as *const c_char);
                if text.is_null() {
                    for j in 0..i {
                        libc::free((*answers.add(j)).text as *mut c_void);
                    }
                    libc::free(answers as *mut c_void);
                    return PAM_BUF_ERR;
                }
                (*answers.add(i)).text = text;
            }
        }
        *responses = answers;
    }
    PAM_SUCCESS
}

/// The user running this, and their full name where the system has one.
pub fn user() -> Option<(String, Option<String>)> {
    // SAFETY: getpwuid's result is read before any other call could
    // replace it.
    unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() {
            return None;
        }
        let name = CStr::from_ptr((*entry).pw_name)
            .to_string_lossy()
            .into_owned();
        let gecos = (*entry).pw_gecos;
        let full = (!gecos.is_null())
            .then(|| CStr::from_ptr(gecos).to_string_lossy().into_owned())
            .and_then(|g| full_name(&g));
        Some((name, full))
    }
}

/// The full name: the first comma-separated field of the GECOS field.
fn full_name(gecos: &str) -> Option<String> {
    let name = gecos.split(',').next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_name_is_the_first_gecos_field() {
        assert_eq!(
            full_name("Divan Visagie,,,").as_deref(),
            Some("Divan Visagie")
        );
        assert_eq!(full_name(",,,"), None);
        assert_eq!(full_name(""), None);
    }

    #[test]
    fn the_running_user_is_known() {
        assert!(user().is_some_and(|(name, _)| !name.is_empty()));
    }

    /// Asks the system's PAM, which takes a few seconds to refuse.
    #[test]
    #[ignore]
    fn a_wrong_password_is_refused() {
        let (name, _) = user().unwrap();
        assert_eq!(authenticate(&name, "not the password"), Ok(false));
    }
}
