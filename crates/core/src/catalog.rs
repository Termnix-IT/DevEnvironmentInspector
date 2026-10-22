use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Runtime,
    AiCli,
    Development,
    PackageManager,
}

/// コマンド名は閉じた許可リスト。カタログから引数やプログラムを注入できない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnownCommand {
    Node,
    Python,
    Python3,
    Java,
    Go,
    Rustc,
    Codex,
    Claude,
    Gemini,
    Git,
    Docker,
    Terraform,
    Aws,
    Kubectl,
    Npm,
    Pnpm,
    Pip,
    Pip3,
    Uv,
    Winget,
}

impl KnownCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Python => "python",
            Self::Python3 => "python3",
            Self::Java => "java",
            Self::Go => "go",
            Self::Rustc => "rustc",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Git => "git",
            Self::Docker => "docker",
            Self::Terraform => "terraform",
            Self::Aws => "aws",
            Self::Kubectl => "kubectl",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Pip => "pip",
            Self::Pip3 => "pip3",
            Self::Uv => "uv",
            Self::Winget => "winget",
        }
    }

    pub fn version_args(self) -> &'static [&'static str] {
        match self {
            Self::Java => &["-version"],
            Self::Go => &["version"],
            Self::Kubectl => &["version", "--client=true", "--output=json"],
            _ => &["--version"],
        }
    }

    /// npm/cmd-shim形式でのみ使用する固定の相対起動先。
    pub fn npm_entrypoint(self) -> Option<&'static str> {
        match self {
            Self::Npm => Some("node_modules/npm/bin/npm-cli.js"),
            Self::Pnpm => Some("node_modules/pnpm/bin/pnpm.cjs"),
            Self::Codex => Some("node_modules/@openai/codex/bin/codex.js"),
            Self::Claude => Some("node_modules/@anthropic-ai/claude-code/cli.js"),
            Self::Gemini => Some("node_modules/@google/gemini-cli/dist/index.js"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub id: String,
    pub name: String,
    pub category: Category,
    pub commands: Vec<KnownCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("カタログJSONが不正です")]
    Json(#[from] serde_json::Error),
    #[error("カタログのスキーマまたは定義が不正です")]
    Invalid,
}

impl Catalog {
    pub fn bundled() -> Result<Self, CatalogError> {
        Self::from_json(include_str!("../../../catalog/tools.json"))
    }

    pub fn from_json(json: &str) -> Result<Self, CatalogError> {
        let catalog: Self = serde_json::from_str(json)?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn validate(&self) -> Result<(), CatalogError> {
        let catalog = self;
        let mut ids = HashSet::new();
        let mut commands = HashSet::new();
        if catalog.schema_version != 1 || catalog.tools.is_empty() || catalog.tools.len() > 64 {
            return Err(CatalogError::Invalid);
        }
        for tool in &catalog.tools {
            if tool.id.is_empty()
                || tool.id.len() > 64
                || !tool
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                || tool.name.is_empty()
                || tool.name.len() > 128
                || tool.name.chars().any(char::is_control)
                || !ids.insert(&tool.id)
                || tool.commands.is_empty()
                || tool.commands.iter().any(|c| !commands.insert(*c))
            {
                return Err(CatalogError::Invalid);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_contains_all_eighteen_tools() {
        assert_eq!(Catalog::bundled().unwrap().tools.len(), 18);
    }

    #[test]
    fn rejects_unknown_commands_and_injected_arguments() {
        for command in ["cmd", "node --eval evil", "../git"] {
            let json = format!(
                r#"{{"schema_version":1,"tools":[{{"id":"x","name":"X","category":"runtime","commands":["{command}"]}}]}}"#
            );
            assert!(Catalog::from_json(&json).is_err());
        }
        let mut json: serde_json::Value =
            serde_json::from_str(include_str!("../../../catalog/tools.json")).unwrap();
        json["tools"][0]["args"] = serde_json::json!(["--eval"]);
        assert!(Catalog::from_json(&json.to_string()).is_err());
    }

    #[test]
    fn rejects_duplicate_identity_and_future_schema() {
        let mut catalog = Catalog::bundled().unwrap();
        catalog.tools.push(catalog.tools[0].clone());
        assert!(Catalog::from_json(&serde_json::to_string(&catalog).unwrap()).is_err());
        catalog.tools.pop();
        catalog.schema_version = 2;
        assert!(Catalog::from_json(&serde_json::to_string(&catalog).unwrap()).is_err());
    }
}
