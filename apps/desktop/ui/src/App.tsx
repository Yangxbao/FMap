/** 应用根组件：路由 + 后端状态加载。 */
import { useEffect, useState } from "react";
import { Layout } from "./components/Layout";
import { Overview } from "./pages/Overview";
import { DiskMap, Files, Tools, Projects, History, Monitor, Reports } from "./pages/Placeholders";
import { Settings } from "./pages/Settings";
import { api, backendAvailable, IpcError, type SystemStatus } from "./lib/ipc";
import { pageFromHash, type PageId } from "./lib/format";

export function App() {
  const [page, setPage] = useState<PageId>(() => pageFromHash(location.hash));
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  // hash 路由（避免引入路由库，M0 够用且依赖最小）
  useEffect(() => {
    const onHash = () => setPage(pageFromHash(location.hash));
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  // 启动时获取真实系统状态（失败如实展示错误）
  useEffect(() => {
    if (!backendAvailable) {
      setError("当前在浏览器中直接打开 UI（无 Tauri 运行时）。");
      return;
    }
    api
      .getSystemStatus()
      .then(setStatus)
      .catch((e: unknown) => setError(e instanceof IpcError ? e.message : String(e)));
  }, []);

  const navigate = (p: PageId) => {
    location.hash = `#/${p}`;
    setPage(p);
  };

  return (
    <Layout page={page} onNavigate={navigate} app={status?.app ?? null} backendConnected={status !== null}>
      {page === "overview" && <Overview status={status} error={error} />}
      {page === "diskmap" && <DiskMap />}
      {page === "files" && <Files />}
      {page === "tools" && <Tools />}
      {page === "projects" && <Projects />}
      {page === "history" && <History />}
      {page === "monitor" && <Monitor />}
      {page === "reports" && <Reports />}
      {page === "settings" && <Settings />}
    </Layout>
  );
}
