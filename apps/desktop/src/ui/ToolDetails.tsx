import type { SourceEvidence, ToolDefinition, ToolScan } from '../api/contracts';
import { Diagnostics } from './Diagnostics';
import { categories, dateTime, installMethods, providers, statuses, versionStatuses } from './labels';

function Sources({ sources }: { sources: SourceEvidence[] }) {
  return <ul>{sources.map((source, index) => <li key={`${source.provider}-${index}`}>
    {providers[source.provider]} / 申告バージョン：{source.reported_version ?? '不明'}
    {source.install_method && <> / 導入方式：{installMethods[source.install_method]}</>}
  </li>)}</ul>;
}

export function ToolDetails({ tool, scan }: { tool?: ToolDefinition; scan?: ToolScan }) {
  return <section aria-labelledby="detail-heading" className="details">
    <h2 id="detail-heading">ツール詳細{tool ? `：${tool.name}` : ''}</h2>
    {!tool ? <p>一覧からツールを選択してください。</p> : <>
      <p>{categories[tool.category]} / コマンド：<code>{tool.commands.join(', ')}</code></p>
      {!scan ? <p>未スキャンです。スキャンを実行すると実行候補と製品登録を表示します。</p> : <>
        <p>検出状態：{statuses[scan.status]}。登録や候補の存在だけで、CLIの利用可否は確定しません。</p>
        <h3>診断</h3>
        <Diagnostics items={scan.diagnostics} />
        <h3>実行候補（{scan.installations.length}件）</h3>
        {!scan.installations.length && <p>確認できた実行候補はありません。</p>}
        {scan.installations.map((installation) => <article key={installation.id} aria-label={`実行候補 ${installation.id}`}>
          <h4>{installation.command} / {installation.id}</h4>
          <dl>
            <dt>実行ファイル</dt><dd><code>{installation.executable_path ?? 'パスを省略しています'}</code></dd>
            <dt>PATH</dt><dd>{installation.on_path ? `登録あり（解決順 ${Number(installation.path_order) + 1}）` : 'PATH上では未確認'}</dd>
            <dt>アクセス</dt><dd>{({ readable: '読み取り可能', denied: '読み取り拒否', unknown: '不明' })[installation.access]}</dd>
            <dt>実測バージョン</dt><dd>{installation.version.version ?? '未取得'}（{versionStatuses[installation.version.status]}）</dd>
            <dt>取得方法</dt><dd>{installation.version.source === 'npm_entrypoint' ? '既知のnpm起動先（ラッパーは未実行）' : installation.version.source === 'native_executable' ? 'ネイティブ実行' : '未実行'}</dd>
            <dt>導入方式</dt><dd>{installation.install_method ? installMethods[installation.install_method] : '未確定'}</dd>
            <dt>検出日時</dt><dd>{dateTime(installation.detected_at)}</dd>
          </dl>
          <p>情報源と申告値</p><Sources sources={installation.sources} />
        </article>)}
        <h3>製品・パッケージ登録（{scan.registrations.length}件）</h3>
        {!scan.registrations.length && <p>確認できた登録情報はありません。</p>}
        {scan.registrations.map((registration) => <article key={registration.id} aria-label={`製品登録 ${registration.id}`}>
          <h4>{registration.package_id ?? registration.id}</h4>
          <dl>
            <dt>配置先</dt><dd><code>{registration.install_location ?? '不明またはパスを省略しています'}</code></dd>
            <dt>検出日時</dt><dd>{dateTime(registration.detected_at)}</dd>
          </dl>
          <Sources sources={registration.sources} />
        </article>)}
      </>}
    </>}
  </section>;
}
