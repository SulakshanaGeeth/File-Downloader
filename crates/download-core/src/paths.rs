use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use percent_encoding::percent_decode_str;
use reqwest::Url;

use crate::Download;

pub(crate) fn validate_url(input: &str) -> Result<Url> {
    let url = Url::parse(input).context("Invalid download URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "Only HTTP and HTTPS URLs are supported"
    );
    ensure!(url.host_str().is_some(), "Download URL must include a host");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "Credentials in download URLs are not supported"
    );
    Ok(url)
}

pub(crate) fn file_name(url: &Url, explicit: Option<String>) -> Result<String> {
    let name = match explicit {
        Some(name) => name,
        None => match url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
        {
            Some(segment) if !segment.is_empty() => percent_decode_str(segment)
                .decode_utf8()
                .context("URL filename is not valid UTF-8; supply a filename")?
                .into_owned(),
            _ => "download.bin".to_owned(),
        },
    };
    validate_file_name(&name)?;
    // Leave space for suffixes chosen at enqueue time and again at publication.
    ensure!(
        name.len() <= 220 && name.encode_utf16().count() <= 220,
        "Filename is too long (maximum 220 bytes / UTF-16 units)"
    );
    Ok(name)
}

pub(crate) fn validate_file_name(name: &str) -> Result<()> {
    ensure!(
        !name.trim().is_empty() && name != "." && name != "..",
        "Filename must not be empty, '.' or '..'"
    );
    ensure!(
        !name.ends_with([' ', '.']),
        "Filename must not end with a space or dot"
    );
    ensure!(
        !name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)),
        "Filename contains a path separator or an invalid character"
    );
    ensure!(
        name.len() <= 255 && name.encode_utf16().count() <= 255,
        "Filename exceeds the filesystem component limit"
    );
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    ensure!(!device, "Filename is a reserved Windows device name");
    Ok(())
}

pub(crate) fn absolute_directory(path: &Path) -> Result<PathBuf> {
    ensure!(
        !path.as_os_str().is_empty(),
        "Destination directory must not be empty"
    );
    let path = std::path::absolute(path).context("Could not resolve destination directory")?;
    if path.exists() {
        ensure!(
            path.is_dir(),
            "Destination is not a directory: {}",
            path.display()
        );
    }
    Ok(path)
}

pub(crate) fn path_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .context("Paths must be valid Unicode")
}

fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

/// Select a display name; publication still uses an atomic no-clobber operation.
pub(crate) fn available_name(
    directory: &Path,
    desired: &str,
    downloads: &[Download],
    ignore_id: Option<&str>,
) -> Result<String> {
    let desired_path = Path::new(desired);
    let stem = desired_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(desired);
    let extension = desired_path.extension().and_then(|s| s.to_str());
    for number in 0..10_000 {
        let candidate = if number == 0 {
            desired.to_owned()
        } else if let Some(extension) = extension {
            format!("{stem} ({number}).{extension}")
        } else {
            format!("{stem} ({number})")
        };
        let full_path = directory.join(&candidate);
        let reserved = downloads.iter().any(|download| {
            Some(download.id.as_str()) != ignore_id
                && same_path(
                    &Path::new(&download.destination).join(&download.file_name),
                    &full_path,
                )
        });
        // symlink_metadata also sees dangling symlinks, which must not be overwritten.
        let exists = match full_path.symlink_metadata() {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error).context("Could not check destination filename"),
        };
        if !reserved && !exists {
            return Ok(candidate);
        }
    }
    bail!("Could not choose an unused filename after 10,000 attempts")
}
