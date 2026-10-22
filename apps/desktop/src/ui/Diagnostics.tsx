import type { Diagnostic } from '../api/contracts';
import { diagnosticLabels, providers } from './labels';

export function Diagnostics({ items, empty = '診断情報はありません。' }: { items: Diagnostic[]; empty?: string }) {
  if (!items.length) return <p>{empty}</p>;
  return <ul className="diagnostics">{items.map((item, index) => <li key={`${item.code}-${item.installation_id}-${index}`}>
    <strong>{item.class === 'fact' ? '確認済み' : '注意'}・{({ info: '情報', warning: '警告', error: 'エラー' })[item.severity]}</strong>
    {'：'}{diagnosticLabels[item.code] ?? '未対応の診断情報です。'}
    {item.provider && <>（{providers[item.provider]}）</>}
    {item.installation_id && <> [{item.installation_id}]</>}
  </li>)}</ul>;
}
