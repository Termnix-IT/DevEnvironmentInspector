use inspector_core::KnownCommand;

/// バナーを保存せず既知の位置にある短いバージョン文字列だけを返す。
pub fn parse_version(command: KnownCommand, stdout: &[u8], stderr: &[u8]) -> Option<String> {
    if command == KnownCommand::Kubectl {
        let value: serde_json::Value = serde_json::from_slice(stdout).ok()?;
        return version_token(value.get("clientVersion")?.get("gitVersion")?.as_str()?);
    }
    for bytes in [stdout, stderr] {
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        for line in text.lines().take(16) {
            let line = line.trim();
            let token = match command {
                KnownCommand::Node
                | KnownCommand::Npm
                | KnownCommand::Pnpm
                | KnownCommand::Gemini
                | KnownCommand::Claude
                | KnownCommand::Winget => line.split_whitespace().next(),
                KnownCommand::Python | KnownCommand::Python3 => {
                    line.strip_prefix("Python ").and_then(first)
                }
                KnownCommand::Java => line
                    .strip_prefix("openjdk version ")
                    .or_else(|| line.strip_prefix("java version "))
                    .or_else(|| line.strip_prefix("openjdk "))
                    .or_else(|| line.strip_prefix("java "))
                    .and_then(first),
                KnownCommand::Go => line.strip_prefix("go version go").and_then(first),
                KnownCommand::Rustc => line.strip_prefix("rustc ").and_then(first),
                KnownCommand::Codex => line.strip_prefix("codex-cli ").and_then(first),
                KnownCommand::Git => line.strip_prefix("git version ").and_then(first),
                KnownCommand::Docker => line.strip_prefix("Docker version ").and_then(first),
                KnownCommand::Terraform => line.strip_prefix("Terraform ").and_then(first),
                KnownCommand::Aws => line.strip_prefix("aws-cli/").and_then(first),
                KnownCommand::Pip | KnownCommand::Pip3 => line.strip_prefix("pip ").and_then(first),
                KnownCommand::Uv => line.strip_prefix("uv ").and_then(first),
                KnownCommand::Kubectl => unreachable!(),
            };
            if let Some(version) = token.and_then(|t| version_token(t.trim_matches(['"', ',']))) {
                return Some(version);
            }
        }
    }
    None
}

fn first(text: &str) -> Option<&str> {
    text.split_whitespace().next()
}

fn version_token(text: &str) -> Option<String> {
    let value = text.strip_prefix('v').unwrap_or(text);
    if value.len() > 64
        || !value.as_bytes().first()?.is_ascii_digit()
        || !value.contains('.')
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
    {
        return None;
    }
    Some(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_versions_for_every_command_without_retaining_banners() {
        let cases = [
            (KnownCommand::Node, "v22.10.0", "22.10.0"),
            (KnownCommand::Python, "Python 3.13.0", "3.13.0"),
            (KnownCommand::Python3, "Python 3.12.8", "3.12.8"),
            (
                KnownCommand::Java,
                "openjdk version \"21.0.4\" 2024-07-16",
                "21.0.4",
            ),
            (
                KnownCommand::Go,
                "go version go1.23.2 windows/amd64",
                "1.23.2",
            ),
            (
                KnownCommand::Rustc,
                "rustc 1.95.0 (abcdef 2026-04-14)",
                "1.95.0",
            ),
            (KnownCommand::Codex, "codex-cli 0.100.0", "0.100.0"),
            (KnownCommand::Claude, "2.0.0 (Claude Code)", "2.0.0"),
            (KnownCommand::Gemini, "0.20.0", "0.20.0"),
            (
                KnownCommand::Git,
                "git version 2.50.0.windows.1",
                "2.50.0.windows.1",
            ),
            (
                KnownCommand::Docker,
                "Docker version 28.5.0, build abc",
                "28.5.0",
            ),
            (KnownCommand::Terraform, "Terraform v1.13.0", "1.13.0"),
            (
                KnownCommand::Aws,
                "aws-cli/2.30.0 Python/3.13.0 Windows/11",
                "2.30.0",
            ),
            (
                KnownCommand::Kubectl,
                r#"{"clientVersion":{"gitVersion":"v1.34.0"}}"#,
                "1.34.0",
            ),
            (KnownCommand::Npm, "11.0.0", "11.0.0"),
            (KnownCommand::Pnpm, "10.0.0", "10.0.0"),
            (
                KnownCommand::Pip,
                "pip 25.0 from C:/Users/private/abc (python 3.13)",
                "25.0",
            ),
            (
                KnownCommand::Pip3,
                "pip 25.0 from C:/Users/private/abc",
                "25.0",
            ),
            (KnownCommand::Uv, "uv 0.8.0 (abc)", "0.8.0"),
            (KnownCommand::Winget, "v1.11.0", "1.11.0"),
        ];
        for (command, text, expected) in cases {
            assert_eq!(
                parse_version(command, text.as_bytes(), b""),
                Some(expected.to_string()),
                "{command:?}"
            );
        }
    }

    #[test]
    fn accepts_java_on_stderr_and_rejects_unrecognized_or_control_output() {
        assert_eq!(
            parse_version(KnownCommand::Java, b"", b"java version \"1.8.0_441\""),
            Some("1.8.0_441".into())
        );
        for text in [
            "token=secret",
            "v22.0.0/secret",
            "\u{1b}[31mv22.0.0",
            "run this command",
        ] {
            assert_eq!(
                parse_version(KnownCommand::Node, text.as_bytes(), b""),
                None
            );
        }
    }
}
