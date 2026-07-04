import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import {
  Check,
  ChevronUp,
  CircleDot,
  ClipboardList,
  Minus,
  Play,
  Plus,
  RefreshCcw,
  Trash2,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

export type IslandMode = "collapsed" | "expanded";

type EditorMode = "layout" | null;

type TodoItem = {
  id: string;
  title: string;
  completed: boolean;
  createdAt: number;
};

type IslandSettings = {
  opacity: number;
  sizeScale: number;
  marginY: number;
};

type IslandShellProps = {
  mode: IslandMode;
  editor: EditorMode;
  activeTaskTitle: string | null;
  onToggle: () => void;
  onCollapse: () => void;
  onMinimize: () => void;
  onEditorChange: (editor: EditorMode) => void;
  children: ReactNode;
};

const STORAGE_KEY = "focusd-island-settings";
const TODOS_STORAGE_KEY = "focusd-island-todos";
const ACTIVE_TODO_STORAGE_KEY = "focusd-island-active-todo";
const BASE_EXPANDED_ISLAND_HEIGHT = 306;
const TODO_ROW_HEIGHT = 46;
const TODO_GROW_START_ROWS = 2;
const TODO_SCROLL_START_ROWS = 6;
const DEFAULT_SETTINGS: IslandSettings = {
  opacity: 100,
  sizeScale: 1,
  marginY: 12,
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
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

function loadTodos(): TodoItem[] {
  const stored = window.localStorage.getItem(TODOS_STORAGE_KEY);

  if (!stored) {
    return [];
  }

  try {
    const parsed = JSON.parse(stored) as Partial<TodoItem>[];

    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed
      .filter((todo) => typeof todo.title === "string" && todo.title.trim())
      .map((todo) => ({
        id:
          typeof todo.id === "string" && todo.id
            ? todo.id
            : createTodoId(),
        title: todo.title?.trim() ?? "",
        completed: Boolean(todo.completed),
        createdAt:
          typeof todo.createdAt === "number" ? todo.createdAt : Date.now(),
      }));
  } catch {
    return [];
  }
}

function loadActiveTodoId() {
  return window.localStorage.getItem(ACTIVE_TODO_STORAGE_KEY);
}

function createTodoId() {
  if ("crypto" in window && typeof window.crypto.randomUUID === "function") {
    return window.crypto.randomUUID();
  }

  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function IslandShell({
  mode,
  editor,
  activeTaskTitle,
  onToggle,
  onCollapse,
  onMinimize,
  onEditorChange,
  children,
}: IslandShellProps) {
  const isExpanded = mode === "expanded";
  const className = [
    "island",
    `island--${mode}`,
    editor === null ? "island--todo" : "island--editor",
  ].join(" ");
  const collapsedLabel = activeTaskTitle
    ? `正在专注：${activeTaskTitle}`
    : "FocuSD Island";

  return (
    <section
      className={className}
      aria-label={collapsedLabel}
      onClick={() => {
        if (!isExpanded) {
          onToggle();
        }
      }}
    >
      <div className="island__collapsed" aria-hidden={isExpanded}>
        <span className="island__pulse" />
        {activeTaskTitle ? (
          <>
            <span className="island__active-task">{activeTaskTitle}</span>
            <span className="island__status">Focus</span>
          </>
        ) : (
          <>
            <span className="island__brand">FocuSD</span>
            <span className="island__status">Ready</span>
          </>
        )}
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
                className={`dot-button dot-button--todo ${
                  editor === null ? "dot-button--active" : ""
                }`}
                type="button"
                title="任务清单"
                aria-label="任务清单"
                onClick={(event) => {
                  event.stopPropagation();
                  onEditorChange(null);
                }}
              />
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

function TodoNotebook({
  todos,
  draft,
  activeTodoId,
  onDraftChange,
  onAddTodo,
  onToggleTodo,
  onStartTodo,
  onDeleteTodo,
}: {
  todos: TodoItem[];
  draft: string;
  activeTodoId: string | null;
  onDraftChange: (value: string) => void;
  onAddTodo: () => void;
  onToggleTodo: (id: string) => void;
  onStartTodo: (id: string) => void;
  onDeleteTodo: (id: string) => void;
}) {
  const openCount = todos.filter((todo) => !todo.completed).length;
  const listClassName = [
    "todo-list",
    todos.length > TODO_SCROLL_START_ROWS ? "todo-list--scroll" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <section className="todo-notebook" aria-label="任务清单">
      <div className="todo-notebook__spine" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>

      <div className="todo-notebook__topline">
        <span className="todo-notebook__tab">
          <ClipboardList size={15} strokeWidth={2.1} />
          Tasks
        </span>
        <span>{openCount} open</span>
      </div>

      <form
        className="todo-form"
        onSubmit={(event) => {
          event.preventDefault();
          onAddTodo();
        }}
      >
        <Plus size={16} strokeWidth={2.2} aria-hidden="true" />
        <input
          value={draft}
          placeholder="Add a task"
          aria-label="Add a task, press Enter to save"
          onChange={(event) => onDraftChange(event.currentTarget.value)}
        />
      </form>

      <div className={listClassName} role="list">
        {todos.length === 0 ? (
          <div className="todo-empty">今天还很轻</div>
        ) : (
          todos.map((todo) => {
            const isActive = todo.id === activeTodoId && !todo.completed;

            return (
              <div
                className={[
                  "todo-item",
                  todo.completed ? "todo-item--done" : "",
                  isActive ? "todo-item--active" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                key={todo.id}
                role="listitem"
              >
                <button
                  className="todo-check"
                  type="button"
                  aria-pressed={todo.completed}
                  title={todo.completed ? "标记未完成" : "完成"}
                  aria-label={`${todo.completed ? "标记未完成" : "完成"}：${
                    todo.title
                  }`}
                  onClick={() => onToggleTodo(todo.id)}
                >
                  {todo.completed && <Check size={14} strokeWidth={2.5} />}
                </button>
                <span className="todo-title">{todo.title}</span>
                <button
                  className="todo-start"
                  type="button"
                  title="开始"
                  aria-label={`开始：${todo.title}`}
                  disabled={todo.completed}
                  onClick={() => onStartTodo(todo.id)}
                >
                  <Play size={13} strokeWidth={2.4} />
                  <span>开始</span>
                </button>
                <button
                  className="todo-delete"
                  type="button"
                  title="删除"
                  aria-label={`删除：${todo.title}`}
                  onClick={() => onDeleteTodo(todo.id)}
                >
                  <Trash2 size={14} strokeWidth={2.2} />
                </button>
              </div>
            );
          })
        )}
      </div>
    </section>
  );
}

function App() {
  const [mode, setMode] = useState<IslandMode>("collapsed");
  const [editor, setEditor] = useState<EditorMode>(null);
  const [settings, setSettings] = useState<IslandSettings>(loadSettings);
  const [todos, setTodos] = useState<TodoItem[]>(loadTodos);
  const [draftTodo, setDraftTodo] = useState("");
  const [activeTodoId, setActiveTodoId] = useState<string | null>(
    loadActiveTodoId,
  );
  const visibleTodoRows = Math.min(
    Math.max(todos.length, 1),
    TODO_SCROLL_START_ROWS,
  );
  const expandedIslandHeight =
    editor === null
      ? BASE_EXPANDED_ISLAND_HEIGHT +
        Math.max(0, visibleTodoRows - TODO_GROW_START_ROWS) * TODO_ROW_HEIGHT
      : BASE_EXPANDED_ISLAND_HEIGHT;
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
        "--expanded-island-height": `${expandedIslandHeight}px`,
      }) as CSSProperties,
    [expandedIslandHeight, settings.opacity, settings.sizeScale],
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

  const syncNativeInteraction = useCallback(
    async (
      nextMode: IslandMode,
      nextSettings: IslandSettings,
      nextExpandedHeight: number,
    ) => {
      try {
        await invoke("set_island_interaction", {
          mode: nextMode,
          sizeScale: nextSettings.sizeScale,
          expandedHeight: nextExpandedHeight,
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

  const addTodo = useCallback(() => {
    const title = draftTodo.trim();

    if (!title) {
      return;
    }

    setTodos((currentTodos) => [
      {
        id: createTodoId(),
        title,
        completed: false,
        createdAt: Date.now(),
      },
      ...currentTodos,
    ]);
    setDraftTodo("");
  }, [draftTodo]);

  const toggleTodo = useCallback((id: string) => {
    setTodos((currentTodos) =>
      currentTodos.map((todo) =>
        todo.id === id ? { ...todo, completed: !todo.completed } : todo,
      ),
    );
    setActiveTodoId((currentId) => (currentId === id ? null : currentId));
  }, []);

  const startTodo = useCallback(
    (id: string) => {
      const todo = todos.find((item) => item.id === id);

      if (!todo || todo.completed) {
        return;
      }

      setActiveTodoId(id);
      setIslandMode("collapsed");
    },
    [setIslandMode, todos],
  );

  const deleteTodo = useCallback((id: string) => {
    setTodos((currentTodos) => currentTodos.filter((todo) => todo.id !== id));
    setActiveTodoId((currentId) => (currentId === id ? null : currentId));
  }, []);

  const resetSettings = useCallback(() => {
    setSettings(DEFAULT_SETTINGS);
    scheduleNativeLayout(DEFAULT_SETTINGS);
  }, [scheduleNativeLayout]);

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  }, [settings]);

  useEffect(() => {
    window.localStorage.setItem(TODOS_STORAGE_KEY, JSON.stringify(todos));
  }, [todos]);

  useEffect(() => {
    if (activeTodoId) {
      window.localStorage.setItem(ACTIVE_TODO_STORAGE_KEY, activeTodoId);
      return;
    }

    window.localStorage.removeItem(ACTIVE_TODO_STORAGE_KEY);
  }, [activeTodoId]);

  useEffect(() => {
    if (
      activeTodoId &&
      !todos.some((todo) => todo.id === activeTodoId && !todo.completed)
    ) {
      setActiveTodoId(null);
    }
  }, [activeTodoId, todos]);

  useEffect(() => {
    scheduleNativeLayout(settings);
  }, [settings.marginY, scheduleNativeLayout]);

  useEffect(() => {
    void syncNativeInteraction(mode, settings, expandedIslandHeight);
  }, [
    expandedIslandHeight,
    mode,
    settings.sizeScale,
    syncNativeInteraction,
  ]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        collapseIsland();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [collapseIsland]);

  const activeTaskTitle = useMemo(() => {
    const activeTodo = todos.find(
      (todo) => todo.id === activeTodoId && !todo.completed,
    );

    return activeTodo?.title ?? null;
  }, [activeTodoId, todos]);

  return (
    <main className="stage" style={stageStyle}>
      <IslandShell
        mode={mode}
        editor={editor}
        activeTaskTitle={activeTaskTitle}
        onToggle={toggleIsland}
        onCollapse={collapseIsland}
        onMinimize={minimizeIsland}
        onEditorChange={setEditor}
      >
        {editor === "layout" && (
          <LayoutEditor
            settings={settings}
            onSettingsChange={setSettings}
            onReset={resetSettings}
          />
        )}
        {editor === null && (
          <TodoNotebook
            todos={todos}
            draft={draftTodo}
            activeTodoId={activeTodoId}
            onDraftChange={setDraftTodo}
            onAddTodo={addTodo}
            onToggleTodo={toggleTodo}
            onStartTodo={startTodo}
            onDeleteTodo={deleteTodo}
          />
        )}
      </IslandShell>
    </main>
  );
}

export default App;
