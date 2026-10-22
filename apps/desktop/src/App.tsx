import { useState } from 'react';
import type { InspectorApi, PathExposure } from './api/contracts';
import { tauriApi } from './api/tauri';
import { useInspector } from './state/useInspector';
import { Diagnostics } from './ui/Diagnostics';
import { ToolDetails } from './ui/ToolDetails';
import { categories, dateTime, providers, statuses, versionSummary } from './ui/labels';

function PathOptions() {
  return <><option value="omit">パスを省略</option><option value="redact_home">ホームを&lt;HOME&gt;に置換</option><option value="full">完全なパスを含める</option></>;
}

export default function App({ api = tauriApi }: { api?: InspectorApi }) {
  const inspector = useInspector(api);
  const [probeVersions, setProbeVersions] = useState(false);
  const [pathOnly, setPathOnly] = useState(false);
  const [exportPaths, setExportPaths] = useState<PathExposure>('omit');
  const snapshot = inspector.view?.snapshot;
  const selectedTool = inspector.catalog?.tools.find((tool) => tool.id === inspector.selectedId);
  const selectedScan = snapshot?.tools.find((scan) => scan.tool.id === inspector.selectedId);
  const detectedCount = snapshot?.tools.filter((tool) => tool.installations.length || tool.registrations.length).length;

  return <main>
    <header>
      <h1>Dev Environment Inspector</h1>
      <p>機能検証用の仮UI · Windowsの開発環境を確認します。</p>
    </header>
    <section aria-label="スキャン操作" className="controls">
      <div className="control-row">
        <button disabled={inspector.busy || !inspector.catalog} onClick={() => void inspector.scan(probeVersions, pathOnly)}>
          {inspector.operation === 'scan' ? 'スキャン中…' : snapshot ? '再スキャン' : 'スキャン実行'}
        </button>
        <label><input type="checkbox" checked={probeVersions} disabled={inspector.busy} onChange={(event) => setProbeVersions(event.target.checked)} /> バージョン取得のためCLIを起動する</label>
        <label><input type="checkbox" checked={pathOnly} disabled={inspector.busy} onChange={(event) => setPathOnly(event.target.checked)} /> PATHのみ探索</label>
      </div>
      <p className="help">既定では外部CLIを実行しません。バージョン取得を選ぶと、既知コマンドを固定引数で起動します。CLI自体の副作用は完全には隔離できません。</p>
      <div className="control-row">
        <label>画面のパス表示 <select value={inspector.paths} disabled={inspector.busy} onChange={(event) => void inspector.changePaths(event.target.value as PathExposure)}><PathOptions /></select></label>
        <label>JSONのパス公開範囲 <select value={exportPaths} disabled={inspector.busy} onChange={(event) => setExportPaths(event.target.value as PathExposure)}><PathOptions /></select></label>
        <button disabled={inspector.busy || !inspector.view} onClick={() => void inspector.exportJson(exportPaths)}>JSONエクスポート</button>
      </div>
      {exportPaths !== 'omit' && <p className="help">JSONには個人情報を含むパスが残る場合があります。保存先・共有先に合わせて公開範囲を選んでください。</p>}
    </section>
    <div role="status" aria-live="polite" className="status">
      {inspector.loadingCatalog ? 'カタログを読み込み中…' : inspector.operation === 'scan' ? '検出処理を実行しています。前回の結果は下に表示しています。' : inspector.operation === 'export' ? '保存先を選択してください。' : inspector.operation === 'paths' ? 'パス表示を変更しています。' : inspector.notice ?? (snapshot ? '直近の検出結果を表示しています。' : 'まだスキャンしていません。')}
    </div>
    {inspector.error && <div role="alert" className="error"><p>{inspector.error}</p>{!inspector.catalog && !inspector.loadingCatalog && <button onClick={inspector.reloadCatalog}>カタログを再読込</button>}</div>}
    {snapshot && <section aria-label="スキャン情報">
      <p>スキャン日時：<time dateTime={snapshot.completed_at}>{dateTime(snapshot.completed_at)}</time> / 検出あり {detectedCount} / 対象 {snapshot.tools.length}</p>
      <p>情報源：{snapshot.providers.map((provider) => providers[provider]).join('、')}</p>
      <details open={snapshot.diagnostics.length > 0}><summary>スキャン全体の診断（{snapshot.diagnostics.length}件）</summary><Diagnostics items={snapshot.diagnostics} /></details>
    </section>}
    <div className="work-area">
      <section aria-labelledby="list-heading">
        <h2 id="list-heading">ツール一覧</h2>
        <div className="table-scroll" tabIndex={0} aria-label="ツール一覧のスクロール領域">
          <table>
            <thead><tr><th scope="col">ツール</th><th scope="col">カテゴリ</th><th scope="col">状態</th><th scope="col">バージョン</th><th scope="col">候補 / 登録</th></tr></thead>
            <tbody>{inspector.catalog?.tools.map((tool) => {
              const scan = snapshot?.tools.find((item) => item.tool.id === tool.id);
              return <tr key={tool.id} data-selected={inspector.selectedId === tool.id}>
                <th scope="row"><button aria-pressed={inspector.selectedId === tool.id} aria-label={`${tool.name}の詳細`} onClick={() => inspector.setSelectedId(tool.id)}>{tool.name}</button></th>
                <td>{categories[tool.category]}</td><td>{scan ? statuses[scan.status] : '未スキャン'}</td>
                <td>{versionSummary(scan)}</td><td>{scan ? `${scan.installations.length} / ${scan.registrations.length}` : '—'}</td>
              </tr>;
            })}</tbody>
          </table>
        </div>
        <p className="help">候補は実行ファイル、登録は製品・パッケージの申告情報です。両方の件数を合計してインストール数と解釈しないでください。未検出は未インストールの断定ではありません。</p>
      </section>
      <ToolDetails tool={selectedTool} scan={selectedScan} />
    </div>
  </main>;
}
