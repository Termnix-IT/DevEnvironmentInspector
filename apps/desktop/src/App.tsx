import { Fragment, useEffect, useRef, useState } from 'react';
import type { Category, InspectorApi, PathExposure } from './api/contracts';
import { tauriApi } from './api/tauri';
import { useInspector } from './state/useInspector';
import { Diagnostics } from './ui/Diagnostics';
import { Icon, ToolMark } from './ui/Icon';
import type { IconName } from './ui/Icon';
import { ToolDetails } from './ui/ToolDetails';
import { categories, dateTime, providers, statuses, versionSummary } from './ui/labels';

type Page = 'inventory' | 'diagnostics' | 'mcp' | 'settings';
const pages: { id: Page; label: string; icon: IconName }[] = [
  { id: 'inventory', label: 'Inventory', icon: 'inventory' },
  { id: 'diagnostics', label: 'Diagnostics', icon: 'diagnostics' },
  { id: 'mcp', label: 'MCP', icon: 'link' },
  { id: 'settings', label: 'Settings', icon: 'settings' },
];
const mcpTools = [
  ['list_tools', '検出された開発ツールの一覧'], ['get_tool_info', 'ツールごとの実行候補・登録・診断'],
  ['check_command', '既知コマンドのPATH解決候補'], ['get_environment_summary', 'カテゴリ別の件数と注意事項'],
  ['scan_environment', '許可範囲内での再スキャン'],
];
function PathOptions() {
  return <><option value="omit">パスを省略</option><option value="redact_home">ホームを&lt;HOME&gt;に置換</option><option value="full">完全なパスを含める</option></>;
}

export default function App({ api = tauriApi }: { api?: InspectorApi }) {
  const inspector = useInspector(api);
  const [page, setPage] = useState<Page>('inventory');
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState<Category | 'all'>('all');
  const [probeVersions, setProbeVersions] = useState(false);
  const [pathOnly, setPathOnly] = useState(false);
  const [exportPaths, setExportPaths] = useState<PathExposure>('omit');
  const search = useRef<HTMLInputElement>(null);
  const focusSearch = useRef(false);
  const actions = useRef<HTMLDetailsElement>(null);
  const snapshot = inspector.view?.snapshot;
  const detectedCount = snapshot?.tools.filter((tool) => tool.installations.length || tool.registrations.length).length ?? 0;
  const notices = snapshot ? [...snapshot.diagnostics, ...snapshot.tools.flatMap((scan) => scan.diagnostics)].filter((item) => item.class === 'caution' || item.severity !== 'info').length : 0;
  const searchTerm = query.trim().toLocaleLowerCase();
  const visibleTools = inspector.catalog?.tools.filter((tool) => (category === 'all' || tool.category === category) && `${tool.name} ${tool.commands.join(' ')}`.toLocaleLowerCase().includes(searchTerm)) ?? [];
  const statusMessage = inspector.loadingCatalog ? 'カタログを読み込み中…' : inspector.operation === 'scan' ? 'スキャン中…' : inspector.operation === 'export' ? '保存先を選択してください。' : inspector.operation === 'paths' ? 'パス表示を変更しています。' : inspector.notice ?? (snapshot ? '検出結果を表示中' : '未スキャン');

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        if (actions.current) actions.current.open = false;
        if (search.current) search.current.focus();
        else { focusSearch.current = true; setPage('inventory'); }
      }
      if (event.key === 'Escape' && actions.current?.open) {
        actions.current.open = false;
        actions.current.querySelector('summary')?.focus();
      }
    }
    function onPointer(event: PointerEvent) {
      if (actions.current && !actions.current.contains(event.target as Node)) actions.current.open = false;
    }
    document.addEventListener('keydown', onKey); document.addEventListener('pointerdown', onPointer);
    return () => { document.removeEventListener('keydown', onKey); document.removeEventListener('pointerdown', onPointer); };
  }, []);
  useEffect(() => {
    if (page === 'inventory' && focusSearch.current) { search.current?.focus(); focusSearch.current = false; }
  }, [page]);

  function navigate(next: Page) { setPage(next); if (actions.current) actions.current.open = false; }
  function exportJson() { if (actions.current) actions.current.open = false; void inspector.exportJson(exportPaths); }
  const scanLabel = inspector.operation === 'scan' ? 'スキャン中…' : snapshot ? '再スキャン' : 'スキャン実行';

  return <div className="app-shell">
    <aside className="sidebar">
      <div className="brand" title="Dev Environment Inspector"><Icon name="brand" /><h1>EnvKit</h1><span className="mcp-badge">MCP</span></div>
      <nav aria-label="メインナビゲーション">{pages.map((item) => <button key={item.id} className="nav-item" aria-current={page === item.id ? 'page' : undefined} onClick={() => navigate(item.id)}><Icon name={item.icon} /><span>{item.label}</span>{item.id === 'diagnostics' && notices > 0 && <span className="nav-count">{notices}</span>}</button>)}</nav>
      <div className="sidebar-caption"><span className="inline-dot neutral" />LOCAL ENVIRONMENT<span>Windows · 読み取り専用</span></div>
    </aside>
    <main className="main-panel">
      <header className="toolbar">
        {page === 'inventory' ? <div className="search-box"><Icon name="search" /><input ref={search} type="search" aria-label="ツールを検索" placeholder="Search tools…" value={query} onChange={(event) => setQuery(event.target.value)} /><kbd>Ctrl K</kbd></div> : <h2 className="page-title">{pages.find((item) => item.id === page)?.label}</h2>}
        <div className="toolbar-actions">
          {page === 'inventory' && <select className="category-filter" aria-label="カテゴリで絞り込み" value={category} onChange={(event) => setCategory(event.target.value as Category | 'all')}><option value="all">All</option>{Object.entries(categories).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>}
          <button className={`icon-button scan-button ${inspector.operation === 'scan' ? 'is-scanning' : ''}`} aria-label={scanLabel} title={`${scanLabel}${probeVersions ? '（CLIでバージョン取得）' : '（CLIは起動しません）'}${pathOnly ? ' · PATHのみ' : ''}`} disabled={inspector.busy || !inspector.catalog} onClick={() => void inspector.scan(probeVersions, pathOnly)}><Icon name="refresh" /></button>
          <details className="actions-menu" ref={actions}><summary className="icon-button" aria-label="その他の操作" title="その他の操作"><Icon name="more" /></summary><div className="actions-popover"><button disabled={inspector.busy || !inspector.view} onClick={exportJson}><Icon name="export" />JSONエクスポート</button><button onClick={() => navigate('settings')}><Icon name="settings" />スキャン・公開設定</button></div></details>
        </div>
      </header>
      {inspector.error && <div role="alert" className="error-banner"><Icon name="diagnostics" /><span>{inspector.error}</span>{!inspector.catalog && !inspector.loadingCatalog && <button onClick={inspector.reloadCatalog}>カタログを再読込</button>}</div>}
      <div className="page-content" key={page}>
        {page === 'inventory' && <section aria-label="ツール一覧" className="inventory">
          {!snapshot && <div className="scan-intro"><span>開発環境を確認する準備ができました。</span><button className="text-button" disabled={inspector.busy || !inspector.catalog} onClick={() => void inspector.scan(probeVersions, pathOnly)}>スキャンを開始 <span aria-hidden="true">↗</span></button></div>}
          <table className="tool-table"><colgroup><col className="tool-column" /><col className="version-column" /><col className="status-column" /><col className="type-column" /><col className="more-column" /></colgroup>
            <thead><tr><th scope="col">Tool</th><th scope="col">Version</th><th scope="col">Status</th><th scope="col">Type</th><th scope="col"><span className="sr-only">詳細</span></th></tr></thead>
            <tbody>{visibleTools.map((tool) => {
              const scan = snapshot?.tools.find((item) => item.tool.id === tool.id);
              const expanded = inspector.selectedId === tool.id;
              const hasCaution = scan?.diagnostics.some((item) => item.severity !== 'info');
              const state = !scan ? 'neutral' : hasCaution || scan.status === 'incomplete' ? 'caution' : scan.status === 'detected' ? 'positive' : 'neutral';
              const stateLabel = !scan ? '未スキャン' : `${statuses[scan.status]}${hasCaution ? '・警告あり' : ''}`;
              return <Fragment key={tool.id}>
                <tr className="tool-row" data-expanded={expanded}>
                  <th scope="row"><button className="tool-toggle" aria-label={`${tool.name}の詳細`} aria-expanded={expanded} aria-controls={expanded ? `details-${tool.id}` : undefined} onClick={() => inspector.setSelectedId(expanded ? null : tool.id)}><Icon name="chevron" className="row-chevron" /><ToolMark id={tool.id} /><span>{tool.name}</span></button></th>
                  <td className="version-cell"><span title={versionSummary(scan)}>{versionSummary(scan)}</span></td>
                  <td><span className={`status-dot ${state}`} role="img" aria-label={stateLabel} title={stateLabel} /></td>
                  <td><span className="type-badge">{categories[tool.category]}</span></td>
                  <td><button className="row-more" aria-label={`${tool.name}の${expanded ? '詳細を閉じる' : 'その他の情報'}`} onClick={() => inspector.setSelectedId(expanded ? null : tool.id)}><Icon name="more" /></button></td>
                </tr>
                {expanded && <tr className="expanded-row"><td colSpan={5}><ToolDetails tool={tool} scan={scan} /></td></tr>}
              </Fragment>;
            })}</tbody>
          </table>
          {!visibleTools.length && !inspector.loadingCatalog && inspector.catalog && <div className="empty-state"><Icon name="search" /><h3>一致するツールがありません</h3><p>ツール名やコマンド名で検索できます。</p><button className="quiet-button" onClick={() => { setQuery(''); setCategory('all'); }}>絞り込みを解除</button></div>}
          <p className="inventory-note">{visibleTools.length} / {inspector.catalog?.tools.length ?? 0} tools<span>未検出は未インストールの断定ではありません。</span></p>
        </section>}
        {page === 'diagnostics' && <section className="document-page" aria-label="環境診断"><div className="section-heading"><Icon name="diagnostics" /><div><h2>環境診断</h2><p>確認できた事実と、未確認・注意が必要な情報。</p></div></div>
          {!snapshot ? <div className="empty-state"><p>スキャン後に診断結果を表示します。</p></div> : <>
            <section className="diagnostic-group"><h3>スキャン全体 <span className="count-badge">{snapshot.diagnostics.length}</span></h3><Diagnostics items={snapshot.diagnostics} /><p className="help">情報源：{snapshot.providers.map((provider) => providers[provider]).join(' · ')}</p></section>
            {snapshot.tools.filter((scan) => scan.diagnostics.length > 0).map((scan) => <section className="diagnostic-group" key={scan.tool.id}><h3><ToolMark id={scan.tool.id} />{scan.tool.name}<button className="text-button" onClick={() => { setQuery(''); setCategory('all'); inspector.setSelectedId(scan.tool.id); navigate('inventory'); }}>詳細を表示 <Icon name="chevron" /></button></h3><Diagnostics items={scan.diagnostics} /></section>)}
            {!snapshot.tools.some((scan) => scan.diagnostics.length) && <p className="muted">ツール別の診断情報はありません。</p>}
          </>}
        </section>}
        {page === 'mcp' && <section className="document-page" aria-label="MCP連携"><div className="section-heading"><Icon name="link" /><div><h2>AIエージェントから環境を確認</h2><p>同じ検出エンジンを、独立したstdioサーバーで利用できます。</p></div></div>
          <div className="connection-info"><Icon name="terminal" /><div><h3>dei-mcp.exe <span className="type-badge">stdio</span></h3><p>GUIを閉じても、MCPクライアントから起動できます。</p><p className="help">この画面ではクライアントの接続状態は監視していません。GUIとサーバーのスキャン結果はそれぞれのプロセスで保持します。</p></div></div>
          <h3 className="subheading">公開ツール <span className="count-badge">5</span></h3><dl className="mcp-tools">{mcpTools.map(([name, description]) => <Fragment key={name}><dt><code>{name}</code></dt><dd>{description}</dd></Fragment>)}</dl>
          <h3 className="subheading">接続設定例</h3><p className="help">ビルドしたサーバーの絶対パスを、お使いのクライアントに設定してください。</p><pre className="config-code"><code>{'{\n  "mcpServers": {\n    "dev-environment-inspector": {\n      "command": "C:/path/to/dei-mcp.exe",\n      "args": []\n    }\n  }\n}'}</code></pre><p className="help">読み取り専用の連携です。インストール・更新・削除や任意コマンドの実行は公開しません。</p>
        </section>}
        {page === 'settings' && <section className="document-page settings-page" aria-label="設定"><div className="section-heading"><Icon name="settings" /><div><h2>スキャンと公開範囲</h2><p>この起動中の設定です。変更は次の操作から適用します。</p></div></div>
          <section className="settings-section"><h3>スキャン</h3>
            <label className="setting-option"><input type="checkbox" aria-label="バージョン取得のためCLIを起動する" aria-describedby="probe-help" checked={probeVersions} disabled={inspector.busy} onChange={(event) => setProbeVersions(event.target.checked)} /><span>バージョン取得のためCLIを起動する<small id="probe-help">既知コマンドを固定引数で実行します。既定ではCLIを起動しません。</small></span></label>
            <label className="setting-option"><input type="checkbox" aria-label="PATHのみ探索" aria-describedby="path-help" checked={pathOnly} disabled={inspector.busy} onChange={(event) => setPathOnly(event.target.checked)} /><span>PATHのみ探索<small id="path-help">製品登録や既知ディレクトリの探索を省略します。</small></span></label>
            {probeVersions && <p className="help">CLI自体によるファイルアクセスや副作用は完全には隔離できません。信頼できる環境で使用してください。</p>}
            <button className="quiet-button" disabled={inspector.busy || !inspector.catalog} onClick={() => { navigate('inventory'); void inspector.scan(probeVersions, pathOnly); }}><Icon name="refresh" />この設定でスキャン</button>
          </section>
          <section className="settings-section"><h3>パスの表示とエクスポート</h3><label className="setting-select">画面のパス表示<select value={inspector.paths} disabled={inspector.busy} onChange={(event) => void inspector.changePaths(event.target.value as PathExposure)}><PathOptions /></select></label><p className="help">取得済みの結果に適用します。変更しても再スキャンは行いません。</p>
            <label className="setting-select">JSONのパス公開範囲<select value={exportPaths} disabled={inspector.busy} onChange={(event) => setExportPaths(event.target.value as PathExposure)}><PathOptions /></select></label>
            {exportPaths !== 'omit' && <p className="help">JSONには個人情報を含むパスが残る場合があります。共有先に合わせて公開範囲を選んでください。</p>}
            <button className="quiet-button" disabled={inspector.busy || !inspector.view} onClick={exportJson}><Icon name="export" />JSONエクスポート</button>
          </section>
        </section>}
      </div>
    </main>
    <footer className="statusbar"><button className="statusbar-mcp" title="MCP連携の設定と仕様" onClick={() => navigate('mcp')}><Icon name="link" />MCP / stdio</button><span className="statusbar-count">{snapshot ? `${detectedCount} tools · ${notices} notices` : `${inspector.catalog?.tools.length ?? '—'} tools`}</span><span className="statusbar-message" role="status" aria-live="polite">{statusMessage}</span><span className="statusbar-time"><Icon name="clock" /><span>Last scanned</span><time dateTime={snapshot?.completed_at}>{snapshot ? dateTime(snapshot.completed_at) : '—'}</time></span></footer>
  </div>;
}
