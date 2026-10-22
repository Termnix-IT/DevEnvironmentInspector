import type { ScanSnapshot } from '../api/contracts';
import { diagnosticLabels, providers } from './labels';
import { diagnosticArea } from './toolPresentation';

export function DiagnosticTable({ snapshot, onSelectTool }: { snapshot: ScanSnapshot; onSelectTool(id: string): void }) {
  const rows = [
    ...snapshot.diagnostics.map((item) => ({ item, tool: null })),
    ...snapshot.tools.flatMap(({ tool, diagnostics }) => diagnostics.map((item) => ({ item, tool }))),
  ];
  const priority = { error: 0, warning: 1, info: 2 };
  rows.sort((a, b) => priority[a.item.severity] - priority[b.item.severity]);
  if (!rows.length) return <p className="muted">診断情報はありません。</p>;
  return <table className="diagnostic-table">
    <caption className="sr-only">環境とツールの診断結果</caption>
    <colgroup><col className="diagnostic-level-column" /><col className="diagnostic-tool-column" /><col /><col className="diagnostic-source-column" /></colgroup>
    <thead><tr><th scope="col">分類 / 重要度</th><th scope="col">対象</th><th scope="col">診断内容</th><th scope="col">情報源 / 候補</th></tr></thead>
    <tbody>{rows.map(({ item, tool }, index) => <tr key={`${tool?.id}-${item.code}-${item.installation_id}-${index}`}>
      <td><span className={`diagnostic-level severity-${item.severity}`}>{item.class === 'fact' ? '確認済み' : '注意'}・{({ info: '情報', warning: '警告', error: 'エラー' })[item.severity]}</span></td>
      <td>{tool ? <button className="diagnostic-tool" aria-label={`${tool.name}の詳細を表示`} onClick={() => onSelectTool(tool.id)}>{tool.name}</button> : 'Environment'}</td>
      <td><span className="diagnostic-area">{diagnosticArea(item.code)}</span>{diagnosticLabels[item.code] ?? '未対応の診断情報です。'}</td>
      <td className="diagnostic-source">{item.provider && providers[item.provider]}{item.provider && item.installation_id && ' · '}{item.installation_id && <code>{item.installation_id}</code>}{!item.provider && !item.installation_id && '—'}</td>
    </tr>)}</tbody>
  </table>;
}
