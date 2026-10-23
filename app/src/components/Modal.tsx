import { useEffect, useId, useRef, type ReactNode } from "react";
import Icon from "./Icon";

export default function Modal({ open, title, onClose, children, className = "" }: {
  open: boolean; title: string; onClose: () => void; children: ReactNode; className?: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);
  return <dialog ref={ref} className={`modal ${className}`} aria-labelledby={titleId}
    onCancel={event => { event.preventDefault(); onClose(); }}
    onClick={event => {
      if (event.target !== event.currentTarget) return;
      const rect = event.currentTarget.getBoundingClientRect();
      if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) onClose();
    }}>
    <header className="modal-head"><div><span className="eyebrow">ASTRAL / FOREST TALE</span><h2 id={titleId}>{title}</h2></div><button className="icon-button" onClick={onClose} aria-label={`关闭${title}`}><Icon name="close" /></button></header>
    {children}
  </dialog>;
}
