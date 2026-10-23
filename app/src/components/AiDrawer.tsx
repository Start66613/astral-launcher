import { useRef, useState } from "react";
import Icon from "./Icon";
import Modal from "./Modal";

export default function AiDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const input = useRef<HTMLInputElement>(null);
  const generation = useRef(0);
  const [fileName, setFileName] = useState("");
  const [preview, setPreview] = useState("");
  const [readError, setReadError] = useState("");
  const [reading, setReading] = useState(false);
  async function readFile(file: File | undefined) {
    if (!file) return;
    const current = ++generation.current;
    setReadError(""); setPreview(""); setFileName("");
    if (!/\.(log|txt)$/i.test(file.name)) { setReadError("请选择 .log 或 .txt 格式的日志。"); return; }
    if (file.size > 2 * 1024 * 1024) { setReadError("日志超过 2 MB，请先截取需要查看的片段。"); return; }
    setReading(true);
    try {
      const text = await file.text();
      if (generation.current !== current) return;
      if (!text.trim()) throw new Error("这份日志是空的，请换一份文件。");
      setFileName(file.name); setPreview(text.split(/\r?\n/).slice(0, 60).join("\n").slice(0, 16000));
    } catch (error) { if (generation.current === current) setReadError(String(error)); }
    finally { if (generation.current === current) setReading(false); }
  }
  function clear() { ++generation.current; setFileName(""); setPreview(""); setReadError(""); setReading(false); if (input.current) input.current.value = ""; }
  return <Modal open={open} title="AI 诊断" onClose={onClose} className="ai-drawer">
    <div className="drawer-intro"><span className="large-spark"><Icon name="spark" /></span><h3>先看看日志，<br />再一起寻找原因。</h3><p>所有预览都留在本机。你决定分享什么。</p></div>
    <input ref={input} className="sr-only" tabIndex={-1} type="file" accept=".log,.txt" aria-label="选择本地日志" onChange={event => void readFile(event.target.files?.[0])} />
    {preview ? <section className="log-preview"><div className="log-preview-heading"><Icon name="log" /><b>{fileName}</b><button className="icon-button" onClick={clear} aria-label="清除日志"><Icon name="close" /></button></div><small>最多预览前 60 行 · 未上传</small><pre tabIndex={0} aria-label="本地日志预览">{preview}</pre><button className="text-button" onClick={() => input.current?.click()}>更换日志 <Icon name="arrow" /></button></section> : <div className="log-empty"><Icon name="log" /><h3>{reading ? "正在读取本地日志…" : "还没有导入日志"}</h3><p>选择一份启动或崩溃日志。<br />支持 .log / .txt，最大 2 MB。</p><button className="btn btn-ghost" disabled={reading} onClick={() => input.current?.click()}>选择本地日志</button></div>}
    {readError && <p className="error-message" role="alert">{readError}</p>}
    <details className="privacy-note"><summary>你的日志，始终由你决定</summary><p>当前仅支持本机预览，不会发送给任何模型。后续接入分析服务时，需要先预览、脱敏并确认待发送内容。</p></details>
    <div className="drawer-bottom"><span>分析服务尚未连接</span><button className="btn btn-primary" disabled>开始分析</button></div>
  </Modal>;
}
