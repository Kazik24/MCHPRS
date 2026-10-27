import { MaterialIcon } from "./MaterialIcon";
import { useEffect, useRef, useState } from "react";

export interface MenuItem {
  label: string;
  shortcut?: string;
  action?: () => void;
  children?: MenuItem[];
  divider?: boolean;
  checked?: boolean;
  disabled?: boolean;
}
interface Props {
  x: number;
  y: number;
  items: MenuItem[];
  close: () => void;
}

export function ContextMenu({ x, y, items, close }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [submenu, setSubmenu] = useState<number | null>(null);
  useEffect(() => {
    const dismiss = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) close();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", escape);
    ref.current?.querySelector<HTMLButtonElement>("button")?.focus();
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", escape);
    };
  }, [close]);
  function render(list: MenuItem[], nested = false) {
    return list.map((item, index) =>
      item.divider ? (
        <div className="menu-separator" key={index} role="separator" />
      ) : (
        <div
          key={item.label}
          className="menu-item-wrap"
          onMouseEnter={() => {
            if (!nested) setSubmenu(item.children ? index : null);
          }}
        >
          <button
            role={item.checked === undefined ? "menuitem" : "menuitemradio"}
            aria-checked={item.checked}
            aria-haspopup={!!item.children}
            disabled={item.disabled}
            onClick={() => {
              if (item.children) setSubmenu(submenu === index ? null : index);
              else {
                item.action?.();
                close();
              }
            }}
          >
            <span>
              {item.checked && (
                <>
                  <MaterialIcon name="check" size={16} />{" "}
                </>
              )}
              {item.label}
            </span>
            <span className="menu-key">
              {item.children ? (
                <MaterialIcon name="chevron" size={16} />
              ) : (
                item.shortcut
              )}
            </span>
          </button>
          {item.children && submenu === index && (
            <div
              className="context-menu submenu"
              style={
                x > window.innerWidth / 2
                  ? { left: "auto", right: "calc(100% - 5px)" }
                  : undefined
              }
              role="menu"
            >
              {render(item.children, true)}
            </div>
          )}
        </div>
      ),
    );
  }
  return (
    <div
      ref={ref}
      className="context-menu"
      role="menu"
      aria-label="Waveform actions"
      style={{
        left: Math.min(x, window.innerWidth - 340),
        top: Math.max(
          8,
          Math.min(y, window.innerHeight - items.length * 31 - 20),
        ),
      }}
      onContextMenu={(event) => event.preventDefault()}
      onKeyDown={(event) => {
        if (!["ArrowDown", "ArrowUp"].includes(event.key)) return;
        event.preventDefault();
        const buttons = Array.from(
          ref.current!.querySelectorAll<HTMLButtonElement>(
            "button:not(:disabled)",
          ),
        );
        const current = buttons.indexOf(
          document.activeElement as HTMLButtonElement,
        );
        buttons[
          (current + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) %
            buttons.length
        ]?.focus();
      }}
    >
      {render(items)}
    </div>
  );
}
