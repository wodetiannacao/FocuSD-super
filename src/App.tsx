import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { ChevronUp, CircleDot, Minus, RefreshCcw, Sparkles } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

export type IslandMode = "collapsed" | "expanded";

type EditorMode = "layout" | null;

type IslandSettings = {
  opacity: number;
  sizeScale: number;
  marginY: number;
  glass: boolean;
};

type IslandShellProps = {
  mode: IslandMode;
  editor: EditorMode;
  settings: IslandSettings;
  onToggle: () => void;
  onCollapse: () => void;
  onMinimize: () => void;
  onEditorChange: (editor: EditorMode) => void;
  onGlassToggle: () => void;
  children: ReactNode;
};

const STORAGE_KEY = "focusd-island-settings";
const DEFAULT_SETTINGS: IslandSettings = {
  opacity: 100,
  sizeScale: 1,
  marginY: 12,
  glass: false,
};

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), max);

function loadSettings(): IslandSettings {
  const stored = window.localStorage.getItem(STORAGE_KEY);

  if (!stored) {
    return DEFAULT_SETTINGS;
  }

  try {
    const parsed = JSON.parse(stored) as Partial<IslandSettings> & {
      margin?: number;
    };

    return {
      opacity: clamp(Number(parsed.opacity ?? DEFAULT_SETTINGS.opacity), 50, 100),
      sizeScale: clamp(
        Number(parsed.sizeScale ?? DEFAULT_SETTINGS.sizeScale),
        0.75,
        1.4,
      ),
      marginY: clamp(
        Number(parsed.marginY ?? parsed.margin ?? DEFAULT_SETTINGS.marginY),
        0,
        160,
      ),
      glass: Boolean(parsed.glass ?? DEFAULT_SETTINGS.glass),
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

function IslandShell({
  mode,
  editor,
  settings,
  onToggle,
  onCollapse,
  onMinimize,
  onEditorChange,
  onGlassToggle,
  children,
}: IslandShellProps) {
  const isExpanded = mode === "expanded";
  const className = [
    "island",
    `island--${mode}`,
    settings.glass ? "island--glass" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <section
      className={className}
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

          <div
            className="island__header-center"
            role="button"
            tabIndex={0}
            title="收起岛屿"
            aria-label="收起岛屿"
            onClick={onCollapse}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onCollapse();
              }
            }}
          >
            <div className="editor-dots" aria-label="岛屿编辑">
              <button
                className={`dot-button dot-button--layout ${
                  editor === "layout" ? "dot-button--active" : ""
                }`}
                type="button"
                title="布局编辑"
                aria-label="布局编辑"
                onClick={(event) => {
                  event.stopPropagation();
                  onEditorChange(editor === "layout" ? null : "layout");
                }}
              />
              <button
                className={`dot-button dot-button--glass ${
                  settings.glass ? "dot-button--active" : ""
                }`}
                type="button"
                title="毛玻璃暂存"
                aria-label="毛玻璃暂存"
                onClick={(event) => {
                  event.stopPropagation();
                  onGlassToggle();
                }}
              />
            </div>
          </div>

          <div className="window-actions">
            <button
              className="icon-button"
              type="button"
              title="收起"
              aria-label="收起岛屿"
              onClick={(event) => {
                event.stopPropagation();
                onCollapse();
              }}
            >
              <ChevronUp size={18} strokeWidth={2.2} />
            </button>
            <button
              className="icon-button"
              type="button"
              title="最小化到托盘"
              aria-label="最小化到托盘"
              onClick={(event) => {
                event.stopPropagation();
                onMinimize();
              }}
            >
              <Minus size={18} strokeWidth={2.2} />
            </button>
          </div>
        </header>
        <div className="island__content">{children}</div>
      </div>
    </section>
  );
}

function SliderControl({
  label,
  value,
  min,
  max,
  step,
  suffix,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  suffix: string;
  onChange: (value: number) => void;
}) {
  return (
    <label className="slider-control">
      <span className="slider-control__meta">
        <span>{label}</span>
        <strong>
          {step < 1 ? value.toFixed(2) : Math.round(value)}
          {suffix}
        </strong>
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
      />
    </label>
  );
}

function LayoutEditor({
  settings,
  onSettingsChange,
  onReset,
}: {
  settings: IslandSettings;
  onSettingsChange: (settings: IslandSettings) => void;
  onReset: () => void;
}) {
  return (
    <div className="editor-panel">
      <div className="editor-panel__header">
        <span>布局设置</span>
        <button
          className="reset-button"
          type="button"
          title="恢复默认"
          aria-label="恢复默认"
          onClick={onReset}
        >
          <RefreshCcw size={15} strokeWidth={2.2} />
        </button>
      </div>
      <SliderControl
        label="不透明度"
        value={settings.opacity}
        min={50}
        max={100}
        step={1}
        suffix="%"
        onChange={(opacity) => onSettingsChange({ ...settings, opacity })}
      />
      <SliderControl
        label="整体大小"
        value={settings.sizeScale}
        min={0.75}
        max={1.4}
        step={0.01}
        suffix="x"
        onChange={(sizeScale) => onSettingsChange({ ...settings, sizeScale })}
      />
      <SliderControl
        label="上下边距"
        value={settings.marginY}
        min={0}
        max={160}
        step={1}
        suffix="px"
        onChange={(marginY) => onSettingsChange({ ...settings, marginY })}
      />
    </div>
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
  const [editor, setEditor] = useState<EditorMode>(null);
  const [settings, setSettings] = useState<IslandSettings>(loadSettings);
  const layoutSync = useRef<{
    frame: number | null;
    inFlight: boolean;
    pending: IslandSettings;
    active: IslandSettings;
  }>({
    frame: null,
    inFlight: false,
    pending: settings,
    active: settings,
  });

  const stageStyle = useMemo(
    () =>
      ({
        "--island-opacity": settings.opacity / 100,
        "--island-scale": settings.sizeScale,
      }) as CSSProperties,
    [settings.opacity, settings.sizeScale],
  );

  const syncNativeLayout = useCallback(async (nextSettings: IslandSettings) => {
    try {
      await invoke("set_island_layout", {
        layout: {
          sizeScale: nextSettings.sizeScale,
          marginY: nextSettings.marginY,
        },
      });
    } catch (error) {
      console.error("Failed to sync island layout", error);
    }
  }, []);

  const flushNativeLayout = useCallback(() => {
    const syncState = layoutSync.current;

    if (syncState.inFlight) {
      return;
    }

    const nextSettings = syncState.pending;
    syncState.active = nextSettings;
    syncState.inFlight = true;

    void syncNativeLayout(nextSettings).finally(() => {
      const latestState = layoutSync.current;
      latestState.inFlight = false;

      if (latestState.pending !== latestState.active) {
        latestState.frame = window.requestAnimationFrame(() => {
          latestState.frame = null;
          flushNativeLayout();
        });
      }
    });
  }, [syncNativeLayout]);

  const scheduleNativeLayout = useCallback(
    (nextSettings: IslandSettings) => {
      const syncState = layoutSync.current;
      syncState.pending = nextSettings;

      if (syncState.frame !== null || syncState.inFlight) {
        return;
      }

      syncState.frame = window.requestAnimationFrame(() => {
        syncState.frame = null;
        flushNativeLayout();
      });
    },
    [flushNativeLayout],
  );

  const syncNativeGlass = useCallback(async (enabled: boolean) => {
    try {
      await invoke("set_glass_effect", { enabled });
    } catch (error) {
      console.error("Failed to sync island glass effect", error);
    }
  }, []);

  const syncNativeInteraction = useCallback(
    async (nextMode: IslandMode, nextSettings: IslandSettings) => {
      try {
        await invoke("set_island_interaction", {
          mode: nextMode,
          sizeScale: nextSettings.sizeScale,
        });
      } catch (error) {
        console.error("Failed to sync island interaction", error);
      }
    },
    [],
  );

  const minimizeIsland = useCallback(async () => {
    try {
      await invoke("minimize_island");
    } catch (error) {
      console.error("Failed to minimize island", error);
    }
  }, []);

  const setIslandMode = useCallback((nextMode: IslandMode) => {
    setMode(nextMode);

    if (nextMode === "collapsed") {
      setEditor(null);
    }
  }, []);

  const toggleIsland = useCallback(() => {
    setIslandMode(mode === "collapsed" ? "expanded" : "collapsed");
  }, [mode, setIslandMode]);

  const collapseIsland = useCallback(() => {
    setIslandMode("collapsed");
  }, [setIslandMode]);

  const toggleGlass = useCallback(() => {
    setEditor(null);
    setSettings((currentSettings) => ({
      ...currentSettings,
      glass: !currentSettings.glass,
    }));
  }, []);

  const resetSettings = useCallback(() => {
    setSettings(DEFAULT_SETTINGS);
    scheduleNativeLayout(DEFAULT_SETTINGS);
  }, [scheduleNativeLayout]);

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  }, [settings]);

  useEffect(() => {
    scheduleNativeLayout(settings);
  }, [settings.marginY, scheduleNativeLayout]);

  useEffect(() => {
    void syncNativeInteraction(mode, settings);
  }, [mode, settings.sizeScale, syncNativeInteraction]);

  useEffect(() => {
    void syncNativeGlass(settings.glass);
  }, [settings.glass, syncNativeGlass]);

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
    <main className="stage" style={stageStyle}>
      <IslandShell
        mode={mode}
        editor={editor}
        settings={settings}
        onToggle={toggleIsland}
        onCollapse={collapseIsland}
        onMinimize={minimizeIsland}
        onEditorChange={setEditor}
        onGlassToggle={toggleGlass}
      >
        {editor === "layout" && (
          <LayoutEditor
            settings={settings}
            onSettingsChange={setSettings}
            onReset={resetSettings}
          />
        )}
        {editor === null && <IslandPlaceholder />}
      </IslandShell>
    </main>
  );
}

export default App;
