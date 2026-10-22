use std::path::PathBuf;

use inspector_core::{AccessStatus, Diagnostic, KnownCommand, ProviderKind, SourceEvidence};

#[derive(Debug, Clone)]
pub struct Candidate {
    pub tool_id: String,
    pub command: KnownCommand,
    pub path: PathBuf,
    pub on_path: bool,
    pub path_order: Option<usize>,
    pub access: AccessStatus,
    pub evidence: SourceEvidence,
}

#[derive(Debug, Clone, Default)]
pub struct Discovery {
    pub candidates: Vec<Candidate>,
    pub registrations: Vec<RegistrationObservation>,
    pub diagnostics: Vec<Diagnostic>,
    /// 情報を確認できなかった場合、未検出を「未インストール」と断定しない。
    pub incomplete: bool,
    /// 特定ツールだけ確認できなかった場合に使用する。
    pub incomplete_tools: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RegistrationObservation {
    pub tool_id: String,
    /// プロバイダー間の統合専用。レジストリキー等を公開データへ保存しない。
    pub identity: String,
    pub package_id: Option<String>,
    pub install_location: Option<PathBuf>,
    pub evidence: SourceEvidence,
}

#[derive(Debug)]
pub struct ProviderFailure;

pub trait DiscoveryProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn discover(&self, catalog: &inspector_core::Catalog) -> Result<Discovery, ProviderFailure>;
}
