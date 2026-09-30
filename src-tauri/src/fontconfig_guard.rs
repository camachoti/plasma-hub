//! Linux-only workaround for malformed Fontconfig records that can hang
//! WebKitGTK 2.52.6 while it creates the first web page.

use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_FONTCONFIG_FILE: &str = "/etc/fonts/fonts.conf";

pub struct FontconfigGuard {
    path: PathBuf,
    previous_fontconfig_file: Option<OsString>,
}

impl Drop for FontconfigGuard {
    fn drop(&mut self) {
        match &self.previous_fontconfig_file {
            Some(path) => env::set_var("FONTCONFIG_FILE", path),
            None => env::remove_var("FONTCONFIG_FILE"),
        }

        if let Err(error) = fs::remove_file(&self.path) {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!(
                    "[fontconfig-guard] não foi possível remover a configuração temporária: {error}"
                );
            }
        }
    }
}

/// Detects Fontconfig entries without a family and hides only those font files
/// from this process. WebKitGTK 2.52.6 can otherwise spin forever while
/// building its initial page's font cache.
pub fn install_if_needed() -> Option<FontconfigGuard> {
    match install() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("[fontconfig-guard] falha ao verificar fontes: {error}");
            None
        }
    }
}

fn install() -> io::Result<Option<FontconfigGuard>> {
    let output = match Command::new("fc-list")
        .args(["--format", "%{file}\t%{family}\n"])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            eprintln!("[fontconfig-guard] fc-list não está instalado; verificação ignorada");
            return Ok(None);
        }
        Err(error) => return Err(error),
    };

    if !output.status.success() {
        eprintln!(
            "[fontconfig-guard] fc-list terminou com status {}; verificação ignorada",
            output.status
        );
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut malformed_fonts: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            let (path, family) = line.split_once('\t')?;
            let path = path.trim();
            if path.is_empty() || !family.trim().is_empty() {
                return None;
            }

            Some(path.to_owned())
        })
        .collect();
    malformed_fonts.sort_unstable();
    malformed_fonts.dedup();

    if malformed_fonts.is_empty() {
        return Ok(None);
    }

    let previous_fontconfig_file = env::var_os("FONTCONFIG_FILE");
    let base_config = previous_fontconfig_file
        .as_deref()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FONTCONFIG_FILE));

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let config_path = env::temp_dir().join(format!(
        "plasma-hub-fontconfig-{}-{timestamp}.conf",
        std::process::id()
    ));

    let mut config = OpenOptions::new();
    config.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        config.mode(0o600);
    }

    let mut file = config.open(&config_path)?;
    let result = write_fontconfig_override(&mut file, &base_config, &malformed_fonts);
    if let Err(error) = result {
        let _ = fs::remove_file(&config_path);
        return Err(error);
    }

    env::set_var("FONTCONFIG_FILE", &config_path);
    eprintln!(
        "[fontconfig-guard] ocultando {} fonte(s) sem família apenas neste processo",
        malformed_fonts.len()
    );

    Ok(Some(FontconfigGuard {
        path: config_path,
        previous_fontconfig_file,
    }))
}

fn write_fontconfig_override(
    file: &mut impl Write,
    base_config: &std::path::Path,
    malformed_fonts: &[String],
) -> io::Result<()> {
    let base_config = xml_escape(base_config.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "caminho Fontconfig inválido")
    })?);

    writeln!(file, "<?xml version=\"1.0\"?>")?;
    writeln!(
        file,
        "<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">"
    )?;
    writeln!(file, "<fontconfig>")?;
    writeln!(file, "  <include>{base_config}</include>")?;
    writeln!(file, "  <selectfont><rejectfont>")?;

    for font_path in malformed_fonts {
        let Some(font_path) = PathBuf::from(font_path).to_str().map(xml_escape) else {
            eprintln!("[fontconfig-guard] ignorando caminho de fonte que não é UTF-8");
            continue;
        };
        writeln!(
            file,
            "    <pattern><patelt name=\"file\"><string>{font_path}</string></patelt></pattern>"
        )?;
    }

    writeln!(file, "  </rejectfont></selectfont>")?;
    writeln!(file, "</fontconfig>")?;
    file.flush()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
