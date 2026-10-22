import { useState } from 'react';
import type { PathExposure } from '../api/contracts';
import type { ProjectEnvironment, ProjectFinding, ProjectPackage, ProjectSource } from '../api/projects';
import type { ProjectsController } from '../state/useProjects';
import { dateTime, scanDuration } from './labels';
import { Icon } from './Icon';
import { ProjectEditor } from './ProjectEditor';

const findingLabels: Record<ProjectFinding, string> = {
  access_denied: '読み取り権限、または登録ルートの安全性を確認できませんでした。', read_failed: '既知メタデータの一部を読み取れませんでした。',
  metadata_invalid: 'メタデータの一部を解釈できないか、記録に矛盾があります。', read_limit_reached: '読み取るファイル数・サイズ・列挙件数の上限に到達しました。',
  scan_deadline_reached: 'スキャンの時間予算に到達しました。', linked_path_skipped: 'リンク・junction経由の配置は読み取りませんでした。',
  unsupported_layout: '未対応の配置・ロック形式が含まれています。', interpreter_not_found: '所定の配置にpython.exeを確認できませんでした。',
  system_packages_not_inspected: 'システムパッケージ共有が有効です。共有先のパッケージは未走査です。',
  non_version_specifier_redacted: 'URL・パスなど、バージョン範囲以外の依存指定は内容を省略しました。', dependency_mismatch: '配置情報のバージョンがロックまたは依存指定と一致しません。',
};
const sourceLabels: Record<ProjectSource, string> = { venv_config: 'pyvenv.cfg', python_distribution: '.dist-info', package_manifest: 'package.json', npm_lock: 'package-lock.json', npm_installed: 'node_modules' };
function Findings({ items }: { items: ProjectFinding[] }) {
  return items.length ? <ul className="project-findings" aria-label="プロジェクトの読み取り確認事項">{items.map((item) => <li key={item}>{findingLabels[item] ?? '未対応の確認事項があります。'}</li>)}</ul> : null;
}
function presence(packageInfo: ProjectPackage, python: boolean) {
  if (packageInfo.lock_matches === false) return { tone: 'caution', label: 'ロックと差異' };
  if (packageInfo.declaration_satisfied === false) return { tone: 'caution', label: '宣言と差異' };
  return packageInfo.presence === 'metadata_present' ? { tone: 'positive', label: python ? '導入情報あり' : '配置情報あり' } : { tone: 'neutral', label: '配置未確認' };
}
function Packages({ environment }: { environment: ProjectEnvironment }) {
  const python = environment.kind === 'python_venv';
  return !environment.packages.length ? <p className="muted project-empty-packages">この環境で確認できたパッケージ情報はありません。</p> : <table className={`project-packages ${python ? 'python-packages' : 'node-packages'}`}>
    <caption className="sr-only">{environment.label}のパッケージ情報</caption>
    <colgroup>{(python ? [32, 20, 24, 24] : [25, 17, 17, 17, 24]).map((width, index) => <col key={index} className={`project-col-${width}`} />)}</colgroup>
    <thead><tr><th scope="col">Package</th>{python ? <><th scope="col">導入バージョン</th><th scope="col">情報源</th></> : <><th scope="col">宣言</th><th scope="col">ロック</th><th scope="col">配置確認</th></>}<th scope="col">確認状態</th></tr></thead>
    <tbody>{environment.packages.map((item, index) => {
      const state = presence(item, python);
      const declared = item.declarations.map((declaration) => declaration.requirement ?? '非バージョン指定（省略）').join(' / ');
      return <tr key={`${item.name}-${index}`}><th scope="row">{item.name}</th>{python ? <><td>{item.installed_version ?? '未確認'}</td><td>{item.sources.map((source) => sourceLabels[source]).join(' · ')}</td></> : <><td title={item.declarations.map((declaration) => `${declaration.kind}: ${declaration.requirement ?? '省略'}`).join(' / ')}>{declared || '—'}</td><td>{item.locked_version ?? '未確認'}</td><td>{item.installed_version ?? '未確認'}</td></>}<td><span className={`inline-dot ${state.tone}`} />{state.label}</td></tr>;
    })}</tbody>
  </table>;
}

export function ProjectsPage({ controller, disabled, exportPaths }: { controller: ProjectsController; disabled: boolean; exportPaths: PathExposure }) {
  const [editor, setEditor] = useState<'register' | 'edit' | null>(null);
  const [environmentId, setEnvironmentId] = useState<string | null>(null);
  const project = controller.projects.find((item) => item.id === controller.selectedId);
  const snapshot = controller.view?.snapshot;
  const environment = snapshot?.environments.find((item) => item.id === environmentId) ?? snapshot?.environments[0];
  return <section className="document-page projects-page" aria-label="プロジェクト環境">
    <div className="page-heading"><h2>プロジェクト環境</h2><span className="page-meta">{controller.projects.length} projects</span><button className="quiet-button" disabled={disabled} onClick={() => setEditor('register')}><Icon name="folder" />プロジェクトを登録</button></div>
    {controller.error && !editor && <div role="alert" className="project-error"><span>{controller.error}</span><button className="text-button" disabled={disabled} onClick={() => void controller.reload()}>登録情報を再読み込み</button></div>}
    {controller.loading && <p className="help">登録情報を読み込み中…</p>}
    {!controller.projects.length && !controller.loading ? <div className="empty-state"><Icon name="folder" /><h3>プロジェクトを登録してください</h3><p>選択したフォルダー内のPython venv・npm環境を確認できます。</p></div> : <div className="projects-layout">
      <aside className="project-list" aria-label="登録プロジェクト一覧"><p className="help">登録プロジェクト</p>{controller.projects.map((item) => <button className="project-list-item" key={item.id} aria-pressed={item.id === controller.selectedId} disabled={disabled} onClick={() => { controller.setSelectedId(item.id); setEnvironmentId(null); }}><span>{item.name}</span><small>MCP参照：{item.mcp_allowed ? '許可' : '許可しない'}</small></button>)}</aside>
      {project && <section className="project-detail" aria-label="選択プロジェクトの詳細">
        <div className="project-detail-heading"><h3>{project.name}</h3><span className="type-badge">MCP参照：{project.mcp_allowed ? '許可' : '許可しない'}</span><button className="text-button" disabled={disabled} onClick={() => setEditor('edit')}>登録設定</button></div>
        <p className="project-root"><code>{project.root ?? 'パスを省略しています'}</code></p>
        {!snapshot ? <div className="project-not-scanned"><p className="muted">このプロセスでは未スキャンです。</p><button className="quiet-button" disabled={disabled} onClick={() => void controller.scan()}><Icon name="refresh" />プロジェクトをスキャン</button></div> : <>
          <dl className="scan-metadata project-scan-time" aria-label="プロジェクトのスキャン日時"><div><dt>開始</dt><dd><time dateTime={snapshot.started_at}>{dateTime(snapshot.started_at)}</time></dd></div><div><dt>完了</dt><dd><time dateTime={snapshot.completed_at}>{dateTime(snapshot.completed_at)}</time></dd></div><div><dt>所要時間</dt><dd>{scanDuration(snapshot.started_at, snapshot.completed_at)}</dd></div></dl>
          <Findings items={snapshot.findings} />
          {snapshot.environments.length > 1 && <div className="project-environment-tabs" role="group" aria-label="プロジェクト内の環境">{snapshot.environments.map((item) => <button key={item.id} aria-pressed={environment?.id === item.id} onClick={() => setEnvironmentId(item.id)}>{item.label}</button>)}</div>}
          {!environment ? <p className="project-empty-packages muted">既知の環境を確認できませんでした。未導入の断定ではありません。</p> : <>
            <div className="project-environment-meta"><span>{environment.label}</span><span>{environment.kind === 'python_venv' ? environment.reported_runtime_version ? `Python ${environment.reported_runtime_version}（構成情報）` : 'Pythonバージョン未確認' : environment.declared_runtime_requirement ? `Node.js 要求 ${environment.declared_runtime_requirement}（宣言）` : 'Node.js要求は未確認'}</span><span>配置先：<code>{environment.path ?? 'パスを省略'}</code></span></div>
            <p className="help project-metadata-note">{environment.kind === 'python_venv' ? 'venv内の導入メタデータを確認します。ターミナルの有効化状態・importの成功は未確認です。' : '宣言・ロック・配置確認は別の情報です。配置情報だけでは実行成功を保証しません。'}</p>
            <Packages environment={environment} />
            {environment.kind === 'python_venv' && <p className="help project-metadata-note">システムパッケージ共有：{environment.system_site_packages === null ? '未確認' : environment.system_site_packages ? 'あり（共有先は未走査）' : 'なし'} · Python実行ファイル：{environment.interpreter_present === null ? '未確認' : environment.interpreter_present ? '所定の配置を確認' : '所定の配置で未検出'}</p>}
            <Findings items={environment.findings} />
          </>}
          <div className="project-export"><button className="text-button" disabled={disabled} onClick={() => void controller.exportJson(exportPaths)}><Icon name="export" />プロジェクト環境をJSONエクスポート</button></div>
        </>}
      </section>}
    </div>}
    {editor && <ProjectEditor key={editor} controller={controller} project={editor === 'edit' ? project : undefined} disabled={disabled} onClose={() => setEditor(null)} />}
  </section>;
}
