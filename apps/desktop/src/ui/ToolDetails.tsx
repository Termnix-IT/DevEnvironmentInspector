import { useState } from 'react';
import type { Installation, SourceEvidence, ToolDefinition, ToolScan } from '../api/contracts';
import { Diagnostics } from './Diagnostics';
import { Icon } from './Icon';
import { dateTime, installMethods, providers, statuses, versionStatuses } from './labels';

function Sources({ sources }: { sources: SourceEvidence[] }) {
  return <ul className="source-list">{sources.map((source, index) => <li key={`${source.provider}-${index}`}>
    {providers[source.provider]} <span>· 申告バージョン {source.reported_version ?? '不明'}</span>
    {source.install_method && <> · {installMethods[source.install_method]}</>}
  </li>)}</ul>;
}

function Candidate({ installation }: { installation: Installation }) {
  const [copyState, setCopyState] = useState<'idle' | 'copying' | 'copied' | 'failed'>('idle');
  async function copyPath() {
    if (!installation.executable_path || copyState === 'copying') return;
    setCopyState('copying');
    try {
      // 表示中の公開範囲だけをコピーし、クリップボードの読み取りは行わない。
      await navigator.clipboard.writeText(installation.executable_path);
      setCopyState('copied');
    } catch { setCopyState('failed'); }
  }
  return <article className="candidate" aria-label={`実行候補 ${installation.id}`}>
    <div className="candidate-main">
      <dl>
        <dt>実行ファイル</dt><dd><code>{installation.executable_path ?? 'パスを省略しています'}</code></dd>
        <dt>情報源</dt><dd>{installation.sources.map((s) => providers[s.provider]).join(' · ') || '不明'}</dd>
        <dt>PATH</dt><dd><span className={`inline-dot ${installation.on_path ? 'positive' : 'neutral'}`} />{installation.on_path ? `登録あり（解決順 ${Number(installation.path_order) + 1}）` : 'PATH上では未確認'}</dd>
        <dt>実測バージョン</dt><dd>{installation.version.version ?? '未取得'}（{versionStatuses[installation.version.status]}）</dd>
        <dt>検出日時</dt><dd><time dateTime={installation.detected_at}>{dateTime(installation.detected_at)}</time></dd>
      </dl>
      <div className="candidate-actions">
        <button className="quiet-button" disabled={!installation.executable_path || copyState === 'copying'} onClick={() => void copyPath()}><Icon name={copyState === 'copied' ? 'check' : 'copy'} />{copyState === 'copied' ? 'コピーしました' : 'パスをコピー'}</button>
        {copyState === 'failed' && <p role="status" className="copy-feedback">コピーできませんでした。パスを選択してコピーしてください。</p>}
        {copyState === 'copied' && <span role="status" className="sr-only">表示中のパスをコピーしました。</span>}
      </div>
    </div>
    <details className="evidence-details"><summary>アクセス・取得方法・申告値</summary>
      <dl>
        <dt>コマンド / 候補ID</dt><dd><code>{installation.command} / {installation.id}</code></dd>
        <dt>アクセス</dt><dd>{({ readable: '読み取り可能', denied: '読み取り拒否', unknown: '不明' })[installation.access]}</dd>
        <dt>取得方法</dt><dd>{installation.version.source === 'npm_entrypoint' ? '既知のnpm起動先（ラッパーは未実行）' : installation.version.source === 'native_executable' ? 'ネイティブ実行' : '未実行'}</dd>
        <dt>導入方式</dt><dd>{installation.install_method ? installMethods[installation.install_method] : '未確定'}</dd>
      </dl>
      <Sources sources={installation.sources} />
    </details>
  </article>;
}

export function ToolDetails({ tool, scan }: { tool: ToolDefinition; scan?: ToolScan }) {
  const [first, ...others] = scan?.installations ?? [];
  return <section aria-label={`${tool.name}の詳細情報`} className="tool-details" id={`details-${tool.id}`}>
    <div className="detail-caption"><span><code>{tool.commands.join(' / ')}</code></span><span>{scan ? statuses[scan.status] : '未スキャン'}</span></div>
    {!scan ? <p className="muted">スキャンを実行すると、実行候補・製品登録・バージョンを確認できます。</p> : <>
      {first ? <Candidate key={`${first.id}-${first.executable_path}`} installation={first} /> : <p className="muted">確認できた実行候補はありません。</p>}
      <div className="detail-sections">
        <details className="evidence-details"><summary>実行候補（{scan.installations.length}件）</summary>
          {first && <p className="help">先頭の候補は上に表示しています。複数の候補があること自体は異常ではありません。</p>}
          {others.map((installation) => <Candidate key={`${installation.id}-${installation.executable_path}`} installation={installation} />)}
        </details>
        {scan.registrations.length > 0 && <details className="evidence-details" open={!first}><summary>製品・パッケージ登録（{scan.registrations.length}件）</summary>
          {scan.registrations.map((registration) => <article className="registration" key={registration.id} aria-label={`製品登録 ${registration.id}`}>
            <h3>{registration.package_id ?? '製品登録'}</h3>
            <dl><dt>配置先</dt><dd><code>{registration.install_location ?? '不明またはパスを省略しています'}</code></dd>
              <dt>検出日時</dt><dd>{dateTime(registration.detected_at)}</dd></dl>
            <Sources sources={registration.sources} />
          </article>)}
        </details>}
        <details className="evidence-details"><summary>診断（{scan.diagnostics.length}件）</summary><Diagnostics items={scan.diagnostics} /></details>
      </div>
      <p className="help detail-note">候補・登録の存在だけではCLIの利用可否は確定しません。バージョンの実測と申告を分けて表示しています。</p>
    </>}
  </section>;
}
