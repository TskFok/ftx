mod common;

#[cfg(target_os = "macos")]
mod openssh;
#[cfg(not(target_os = "macos"))]
mod russh;

#[cfg(target_os = "macos")]
pub use openssh::SftpClient;
#[cfg(not(target_os = "macos"))]
pub use russh::SftpClient;
