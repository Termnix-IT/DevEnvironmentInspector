import { useEffect, useRef, useState } from 'react';
import type { PathExposure } from '../api/contracts';
import type { ProjectApi, ProjectSummary, ProjectView, RegisterProjectRequest } from '../api/projects';
import { errorMessage } from './useInspector';

export function useProjects(api: ProjectApi, paths: PathExposure, active: boolean) {
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [listPaths, setListPaths] = useState<PathExposure>(paths);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [view, setView] = useState<ProjectView | null>(null);
  const [viewPaths, setViewPaths] = useState<PathExposure>(paths);
  const [loading, setLoading] = useState(false);
  const [reading, setReading] = useState<string | null>(null);
  const [operation, setOperation] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const inFlight = useRef(false);
  const listSequence = useRef(0);
  const readSequence = useRef(0);
  const selectedProject = projects.find((project) => project.id === selectedId);
  const registrationKey = JSON.stringify(selectedProject ? [selectedProject.id, selectedProject.revision, selectedProject.root, selectedProject.extra_venv, selectedProject.mcp_allowed] : null);

  async function reload(preferred?: string) {
    const sequence = ++listSequence.current;
    setLoading(true); setError(null);
    try {
      const result = await api.list(paths);
      if (sequence !== listSequence.current) return;
      setProjects(result); setListPaths(paths);
      setSelectedId((id) => result.some((p) => p.id === (preferred ?? id)) ? preferred ?? id : result[0]?.id ?? null);
    } catch (failure) {
      if (sequence === listSequence.current) { setError(errorMessage(failure)); setProjects([]); setView(null); }
    } finally { if (sequence === listSequence.current) setLoading(false); }
  }
  useEffect(() => { if (active) void reload(); }, [api, paths, active]);
  useEffect(() => {
    if (!active || !selectedId) return;
    const sequence = ++readSequence.current;
    const key = `${selectedId}:${paths}`;
    setReading(key);
    void api.snapshot({ project_id: selectedId, paths }).then((result) => {
      if (sequence === readSequence.current) { setView(result); setViewPaths(paths); }
    }).catch((failure) => {
      if (sequence === readSequence.current) { setError(errorMessage(failure)); setView(null); }
    }).finally(() => { if (sequence === readSequence.current) setReading(null); });
    return () => { readSequence.current++; };
  }, [api, selectedId, paths, active, registrationKey]);

  async function run<T>(name: string, work: () => Promise<T>): Promise<T | null> {
    if (inFlight.current) return null;
    inFlight.current = true; setOperation(name); setError(null); setNotice(null);
    try { return await work(); }
    catch (failure) { setError(errorMessage(failure)); return null; }
    finally { inFlight.current = false; setOperation(null); }
  }
  const currentView = view?.project.id === selectedId && viewPaths === paths && selectedProject?.revision === view.project.revision && selectedProject.root === view.project.root && selectedProject.extra_venv === view.project.extra_venv ? view : null;
  const visibleProjects = listPaths === paths ? projects : projects.map((p) => ({ ...p, root: null, extra_venv: null }));
  return {
    projects: visibleProjects, selectedId, setSelectedId, view: currentView, loading,
    busy: loading || (active && reading === `${selectedId}:${paths}`) || operation !== null, operation, error, notice,
    reload: () => reload(),
    chooseFolder: (kind: 'project' | 'venv') => run('folder', () => api.chooseFolder({ kind, paths })),
    register: (request: Omit<RegisterProjectRequest, 'paths'>) => run('register', async () => {
      const result = await api.register({ ...request, paths });
      setView(null); await reload(result.id); setNotice('プロジェクトを登録しました。スキャンはまだ実行していません。'); return result;
    }),
    update: (id: string, name: string, allowed: boolean) => run('update', async () => {
      const result = await api.update({ project_id: id, name, mcp_allowed: allowed, paths });
      setView(null); await reload(id); setNotice('登録設定を更新しました。'); return result;
    }),
    remove: (id: string) => run('remove', async () => {
      await api.remove({ project_id: id }); setView(null); await reload(); setNotice('プロジェクトの登録を解除しました。'); return true;
    }),
    scan: () => selectedId ? run('scan', async () => {
      ++readSequence.current; setReading(null);
      const result = await api.scan({ project_id: selectedId, paths });
      setView(result); setViewPaths(paths); setNotice('プロジェクトのスキャンが完了しました。'); return result;
    }) : Promise.resolve(null),
    exportJson: (exposure: PathExposure) => currentView?.scan_revision ? run('export', async () => {
      const result = await api.export({ project_id: currentView.project.id, scan_revision: currentView.scan_revision!, paths: exposure });
      setNotice(result === 'saved' ? 'プロジェクト環境のJSONを保存しました。' : '保存をキャンセルしました。'); return result;
    }) : Promise.resolve(null),
  };
}
export type ProjectsController = ReturnType<typeof useProjects>;
