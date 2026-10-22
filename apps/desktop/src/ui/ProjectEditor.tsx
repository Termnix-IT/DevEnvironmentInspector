import { useEffect, useRef, useState } from 'react';
import type { FolderSelection, ProjectSummary } from '../api/projects';
import type { ProjectsController } from '../state/useProjects';

export function ProjectEditor({ controller, project, disabled, onClose }: { controller: ProjectsController; project?: ProjectSummary; disabled: boolean; onClose(): void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState(project?.name ?? '');
  const [allowed, setAllowed] = useState(project?.mcp_allowed ?? false);
  const [root, setRoot] = useState<FolderSelection | null>(null);
  const [extra, setExtra] = useState<FolderSelection | null>(null);
  const [useExtra, setUseExtra] = useState(false);
  const [validation, setValidation] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);
  useEffect(() => { dialog.current?.showModal(); return () => dialog.current?.close(); }, []);
  async function submit(event: React.FormEvent) {
    event.preventDefault(); setValidation(null);
    if (!name.trim()) { setValidation('表示名を入力してください。'); return; }
    if (project) {
      if (await controller.update(project.id, name.trim(), allowed)) onClose();
    } else {
      if (!root || (useExtra && !extra)) { setValidation('登録するフォルダーを選択してください。'); return; }
      if (await controller.register({ name: name.trim(), root_selection: root.token, extra_venv_selection: useExtra ? extra!.token : null, mcp_allowed: allowed })) onClose();
    }
  }
  return <dialog ref={dialog} className="project-dialog" aria-labelledby="project-editor-title" onCancel={(event) => { event.preventDefault(); if (!disabled) onClose(); }}>
    <form onSubmit={(event) => void submit(event)}>
      <h3 id="project-editor-title">{project ? 'プロジェクトの登録設定' : 'プロジェクトを登録'}</h3>
      {(validation || controller.error) && <p role="alert" className="project-form-error">{validation ?? controller.error}</p>}
      <label className="project-field">表示名<input autoFocus value={name} required maxLength={80} disabled={disabled} onChange={(event) => setName(event.target.value)} /></label>
      <label className="project-field">プロジェクトフォルダー<span className="project-folder-field"><input aria-label="プロジェクトフォルダー" readOnly value={project ? project.root ?? 'パスを省略しています' : root?.path ?? (root ? '選択済み（パスを省略）' : '')} placeholder="フォルダーを選択してください" />{!project && <button type="button" className="quiet-button" aria-label="プロジェクトフォルダーを選択" disabled={disabled} onClick={async () => { const result = await controller.chooseFolder('project'); if (result) setRoot(result); }}>選択</button>}</span></label>
      {!project && <><label className="project-check"><input type="checkbox" checked={useExtra} disabled={disabled} onChange={(event) => setUseExtra(event.target.checked)} />プロジェクト外のvenvを追加</label>
        {useExtra && <label className="project-field">追加のvenvフォルダー<span className="project-folder-field"><input aria-label="追加のvenvフォルダー" readOnly value={extra?.path ?? (extra ? '選択済み（パスを省略）' : '')} placeholder="venvフォルダーを選択してください" /><button type="button" className="quiet-button" aria-label="追加のvenvフォルダーを選択" disabled={disabled} onClick={async () => { const result = await controller.chooseFolder('venv'); if (result) setExtra(result); }}>選択</button></span></label>}</>}
      {project?.extra_venv && <p className="help">追加venv：<code>{project.extra_venv}</code></p>}
      <label className="project-check"><input type="checkbox" checked={allowed} disabled={disabled} onChange={(event) => setAllowed(event.target.checked)} />MCPからこのプロジェクトの参照を許可する</label>
      <p className="help">登録時はスキャンしません。登録情報はこのPCのアプリデータに保存します。</p>
      {removing && <div className="project-remove-confirm"><p>このプロジェクトの登録を解除します。フォルダーの内容は保持されます。</p><button type="button" className="quiet-button" disabled={disabled} onClick={async () => { if (await controller.remove(project!.id)) onClose(); }}>登録を解除する</button></div>}
      <div className="project-dialog-actions">{project && !removing && <button type="button" className="text-button project-remove" disabled={disabled} onClick={() => setRemoving(true)}>登録解除</button>}<button type="button" className="quiet-button" disabled={disabled} onClick={onClose}>キャンセル</button><button type="submit" className="quiet-button" disabled={disabled}>{project ? '設定を保存' : '登録'}</button></div>
    </form>
  </dialog>;
}
