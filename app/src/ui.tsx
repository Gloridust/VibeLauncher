import type { ComponentChildren, JSX } from "preact";
import { createContext } from "preact";
import { useContext, useEffect, useRef, useState } from "preact/hooks";
import { CheckCircle2, AlertCircle } from "lucide-preact";
import type { LucideIcon } from "lucide-preact";
import type { ProfileView, StateView } from "./api";

// ── shared app context ──

export interface Ctx {
  state: StateView;
  refresh: () => Promise<void>;
  toast: (text: string, kind?: "ok" | "error") => void;
  /** Run an action, report its error as a toast; returns whether it worked. */
  run: (fn: () => Promise<unknown>, okText?: string) => Promise<boolean>;
}
export const AppCtx = createContext<Ctx>(null as unknown as Ctx);
export const useApp = () => useContext(AppCtx);

// ── controls ──

export function Switch(props: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean; label?: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.on}
      aria-label={props.label}
      class={"switch" + (props.on ? " on" : "")}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.on)}
    />
  );
}

export function Segmented<T extends string>(props: {
  value: T;
  options: { value: T; label: ComponentChildren; disabled?: boolean }[];
  onChange: (v: T) => void;
  label?: string;
}) {
  return (
    <div class="segmented" role="radiogroup" aria-label={props.label}>
      {props.options.map((o) => (
        <button
          type="button"
          role="radio"
          aria-checked={o.value === props.value}
          class={o.value === props.value ? "on" : ""}
          disabled={o.disabled}
          onClick={() => props.onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export interface MenuItem {
  label?: string;
  icon?: LucideIcon;
  onClick?: () => void;
  danger?: boolean;
  disabled?: boolean;
  separator?: boolean;
}

/** A button that opens a small menu below it. */
export function MenuButton(props: { button: (open: () => void, isOpen: boolean) => JSX.Element; items: MenuItem[]; align?: "left" | "right" }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: Event) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        const btns = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>(".menu button:not(:disabled)") ?? []);
        const i = btns.indexOf(document.activeElement as HTMLButtonElement);
        const n = e.key === "ArrowDown" ? (i + 1) % btns.length : (i - 1 + btns.length) % btns.length;
        btns[n]?.focus();
        e.preventDefault();
      }
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", key);
    requestAnimationFrame(() => ref.current?.querySelector<HTMLButtonElement>(".menu button:not(:disabled)")?.focus());
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <div class="menu-wrap" ref={ref}>
      {props.button(() => setOpen(!open), open)}
      {open && (
        <div class={"menu" + (props.align === "right" ? " right" : "")} role="menu">
          {props.items.map((it, i) =>
            it.separator ? (
              <hr key={i} />
            ) : (
              <button
                key={i}
                type="button"
                role="menuitem"
                class={it.danger ? "danger" : ""}
                disabled={it.disabled}
                onClick={() => {
                  setOpen(false);
                  it.onClick?.();
                }}
              >
                {it.icon && <it.icon />}
                {it.label}
              </button>
            ),
          )}
        </div>
      )}
    </div>
  );
}

/** Modal sheet. Escape and a click outside close it. */
export function Sheet(props: {
  title: ComponentChildren;
  subtitle?: ComponentChildren;
  onClose: () => void;
  children: ComponentChildren;
  footer?: ComponentChildren;
  wide?: boolean;
  /** Submitting the form (Enter in a field) runs this. */
  onSubmit?: () => void;
}) {
  const ref = useRef<HTMLFormElement>(null);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        props.onClose();
      }
    };
    document.addEventListener("keydown", key);
    const prev = document.activeElement as HTMLElement | null;
    requestAnimationFrame(() => {
      const root = ref.current;
      const first =
        root?.querySelector<HTMLElement>("[autofocus]") ?? root?.querySelector<HTMLElement>("input, textarea, .sheet-foot .btn.primary, button");
      first?.focus();
    });
    return () => {
      document.removeEventListener("keydown", key);
      prev?.focus?.();
    };
  }, []);
  return (
    <div
      class="backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      <form
        ref={ref}
        class={"sheet" + (props.wide ? " wide" : "")}
        // A form is the sheet so Enter submits; it is still a dialog.
        {...({ role: "dialog" } as object)}
        aria-modal="true"
        onSubmit={(e) => {
          e.preventDefault();
          props.onSubmit?.();
        }}
      >
        <div class="sheet-head">
          <h2>{props.title}</h2>
          {props.subtitle && <p>{props.subtitle}</p>}
        </div>
        <div class="sheet-body">{props.children}</div>
        {props.footer && <div class="sheet-foot">{props.footer}</div>}
      </form>
    </div>
  );
}

export function ProfileIcon(props: { p: ProfileView; size: number; class?: string }) {
  if (props.p.iconUrl) return <img src={props.p.iconUrl} width={props.size} height={props.size} class={props.class} alt="" draggable={false} />;
  return <span class={"tile " + (props.class ?? "")} style={{ background: props.p.color, width: props.size, height: props.size, display: "inline-block" }} />;
}

// ── toasts ──

export interface Toast {
  id: number;
  text: string;
  kind: "ok" | "error";
}

export function Toasts(props: { items: Toast[] }) {
  return (
    <div class="toasts" aria-live="polite">
      {props.items.map((t) => (
        <div key={t.id} class={"toast" + (t.kind === "error" ? " error" : "")}>
          {t.kind === "error" ? <AlertCircle /> : <CheckCircle2 />}
          <span>{t.text}</span>
        </div>
      ))}
    </div>
  );
}

export const COLORS = ["#D97757", "#E5484D", "#F59E0B", "#2F9E62", "#14B8A6", "#3B82F6", "#6E7BF2", "#A855F7", "#EC4899", "#78716C"];

export function ColorPicker(props: { value: string; onChange: (c: string) => void }) {
  return (
    <div class="swatches" role="radiogroup">
      {COLORS.map((c) => (
        <button
          type="button"
          role="radio"
          aria-checked={c.toLowerCase() === props.value.toLowerCase()}
          aria-label={c}
          class={"swatch" + (c.toLowerCase() === props.value.toLowerCase() ? " on" : "")}
          style={{ background: c }}
          onClick={() => props.onChange(c)}
        />
      ))}
    </div>
  );
}
