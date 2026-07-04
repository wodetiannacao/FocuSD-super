import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ChevronUp, CircleDot, Sparkles } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

export type IslandMode = "collapsed" | "expanded";

type IslandShellProps = {
  mode: IslandMode;
  onToggle: () => void;
  onCollapse: () => void;
  children: ReactNode;
};

function IslandShell({
  mode,
  onToggle,
  onCollapse,
  children,
}: IslandShellProps) {
  const isExpanded = mode === "expanded";

  return (
    <section
      className={`island island--${mode}`}
      aria-label="FocuSD Island"
      onClick={() => {
        if (!isExpanded) {
          onToggle();
        }
      }}
    >
      <div className="island__collapsed" aria-hidden={isExpanded}>
        <span className="island__pulse" />
        <span className="island__brand">FocuSD</span>
        <span className="island__status">Ready</span>
      </div>

      <div className="island__expanded" aria-hidden={!isExpanded}>
        <header className="island__header">
          <div className="island__title">
            <CircleDot size={16} strokeWidth={2.2} />
            <span>FocuSD</span>
          </div>
          <button
            className="icon-button"
            type="button"
            aria-label="Collapse island"
            onClick={(event) => {
              event.stopPropagation();
              onCollapse();
            }}
          >
            <ChevronUp size={18} strokeWidth={2.2} />
          </button>
        </header>
        <div className="island__content">{children}</div>
      </div>
    </section>
  );
}

function IslandPlaceholder() {
  return (
    <div className="placeholder" aria-label="Island content slot">
      <div className="placeholder__badge">
        <Sparkles size={22} strokeWidth={2.1} />
      </div>
      <div className="placeholder__copy">
        <span className="placeholder__eyebrow">Shell online</span>
        <strong>Module slot</strong>
      </div>
      <div className="placeholder__bars" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}

function App() {
  const [mode, setMode] = useState<IslandMode>("collapsed");
  const transitionId = useRef(0);

  const syncNativeMode = useCallback(async (nextMode: IslandMode) => {
    try {
      await invoke("set_island_mode", { mode: nextMode });
    } catch (error) {
      console.error("Failed to resize island window", error);
    }
  }, []);

  const setIslandMode = useCallback(
    (nextMode: IslandMode) => {
      transitionId.current += 1;
      const currentTransition = transitionId.current;

      if (nextMode === "expanded") {
        void syncNativeMode(nextMode).finally(() => {
          if (currentTransition === transitionId.current) {
            setMode(nextMode);
          }
        });
        return;
      }

      setMode(nextMode);
      window.setTimeout(() => {
        if (currentTransition === transitionId.current) {
          void syncNativeMode(nextMode);
        }
      }, 240);
    },
    [syncNativeMode],
  );

  const toggleIsland = useCallback(() => {
    setIslandMode(mode === "collapsed" ? "expanded" : "collapsed");
  }, [mode, setIslandMode]);

  const collapseIsland = useCallback(() => {
    setIslandMode("collapsed");
  }, [setIslandMode]);

  useEffect(() => {
    void syncNativeMode("collapsed");
  }, [syncNativeMode]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        collapseIsland();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [collapseIsland]);

  return (
    <main className="stage">
      <IslandShell
        mode={mode}
        onToggle={toggleIsland}
        onCollapse={collapseIsland}
      >
        <IslandPlaceholder />
      </IslandShell>
    </main>
  );
}

export default App;
