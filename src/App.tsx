import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import logoUrl from "./assets/logo.svg";
import "./App.css";

type LockKeyId = "caps" | "num" | "scroll";
type Language = "en" | "zh";
type OsdPositionId =
  | "top-left"
  | "top-center"
  | "top-right"
  | "middle-left"
  | "middle-center"
  | "middle-right"
  | "bottom-left"
  | "bottom-center"
  | "bottom-right";

type LockChangePayload = {
  key: LockKeyId;
  name: string;
  abbreviation: string;
  icon: LockKeyId;
  enabled: boolean;
};

type StartupToastPayload = {
  title: string;
  message: string;
};

type OsdNotice =
  | {
      kind: "lock";
      payload: LockChangePayload;
    }
  | {
      kind: "toast";
      payload: StartupToastPayload;
    };

type OsdEnabledMap = Record<LockKeyId, boolean>;

declare global {
  interface Window {
    __KEYBOARD_LOCK_OSD_SHOW?: (notice: OsdNotice) => void;
  }
}

const fallbackStates: LockChangePayload[] = [
  {
    key: "caps",
    name: "Caps Lock",
    abbreviation: "CAP",
    icon: "caps",
    enabled: false,
  },
  {
    key: "num",
    name: "Num Lock",
    abbreviation: "NUM",
    icon: "num",
    enabled: false,
  },
  {
    key: "scroll",
    name: "Scroll Lock",
    abbreviation: "SCRL",
    icon: "scroll",
    enabled: false,
  },
];

const defaultOsdEnabled: OsdEnabledMap = {
  caps: true,
  num: true,
  scroll: true,
};

const osdEnabledStorageKey = "keyboard-lock-osd.enabledKeys";
const suppressFullscreenStorageKey = "keyboard-lock-osd.suppressFullscreen";
const osdPositionStorageKey = "keyboard-lock-osd.position";
const themeColorStorageKey = "keyboard-lock-osd.themeColor";
const defaultOsdPosition: OsdPositionId = "bottom-center";
const defaultThemeColor = "#1f9d63";
const themeColorPresets = [
  "#1f9d63",
  "#0ea5e9",
  "#6366f1",
  "#a855f7",
  "#ec4899",
  "#f97316",
  "#eab308",
  "#ef4444",
];
const osdPositions: OsdPositionId[] = [
  "top-left",
  "top-center",
  "top-right",
  "middle-left",
  "middle-center",
  "middle-right",
  "bottom-left",
  "bottom-center",
  "bottom-right",
];
const osdPositionLayout: OsdPositionId[] = [
  "top-left",
  "top-center",
  "top-right",
  "middle-left",
  "middle-center",
  "middle-right",
  "bottom-left",
  "bottom-center",
  "bottom-right",
];

const copy = {
  en: {
    title: "Keyboard Lock OSD",
    subtitle: "Lock key indicators",
    osd: "OSD",
    preview: "Preview",
    status: "Current state",
    on: "ON",
    off: "OFF",
    show: "Show OSD",
    startup: "Startup",
    startAtLogin: "Start at login",
    position: "Position",
    selectPosition: "Choose OSD position",
    positions: {
      "top-left": "Top left",
      "top-center": "Top center",
      "top-right": "Top right",
      "middle-left": "Left center",
      "middle-center": "Center",
      "middle-right": "Right center",
      "bottom-left": "Bottom left",
      "bottom-center": "Bottom center",
      "bottom-right": "Bottom right",
    },
    themeColor: "Theme color",
    selectThemeColor: "Choose theme color",
    customColor: "Custom color",
    hideInFullscreen: "Hide OSD in fullscreen",
  },
  zh: {
    title: "Keyboard Lock OSD",
    subtitle: "锁定键状态提示",
    osd: "屏幕浮层",
    preview: "预览",
    status: "当前状态",
    on: "开",
    off: "关",
    show: "显示浮层",
    startup: "开机启动",
    startAtLogin: "开机自启",
    position: "位置",
    selectPosition: "选择浮层位置",
    positions: {
      "top-left": "左上角",
      "top-center": "顶部居中",
      "top-right": "右上角",
      "middle-left": "左侧居中",
      "middle-center": "屏幕中央",
      "middle-right": "右侧居中",
      "bottom-left": "左下角",
      "bottom-center": "底部居中",
      "bottom-right": "右下角",
    },
    themeColor: "主题颜色",
    selectThemeColor: "选择主题颜色",
    customColor: "自定义颜色",
    hideInFullscreen: "全屏时不显示浮层",
  },
};

function App() {
  const view = new URLSearchParams(window.location.search).get("view");

  if (view === "osd") {
    return <OsdView />;
  }

  return <SettingsView />;
}

function OsdView() {
  const [notice, setNotice] = useState<OsdNotice | null>(null);
  const [visible, setVisible] = useState(false);
  const hideTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    applyThemeColor(readStoredThemeColor());

    const showNotice = (nextNotice: OsdNotice, duration = 1_150) => {
      window.clearTimeout(hideTimer.current);
      setNotice(nextNotice);
      setVisible(true);

      hideTimer.current = window.setTimeout(() => {
        setVisible(false);
      }, duration);
    };

    window.__KEYBOARD_LOCK_OSD_SHOW = (nextNotice) =>
      showNotice(nextNotice, nextNotice.kind === "toast" ? 2_400 : 1_150);

    const setup = async () => {
      const unlistenLock = await listen<LockChangePayload>(
        "lock-key-change",
        (event) => {
          showNotice({ kind: "lock", payload: event.payload });
        },
      );
      const unlistenToast = await listen<StartupToastPayload>(
        "startup-tray-toast",
        (event) => {
          showNotice({ kind: "toast", payload: event.payload }, 2_400);
        },
      );
      const unlistenThemeColor = await listen<string>(
        "osd-theme-color",
        (event) => applyThemeColor(event.payload),
      );

      void invoke("osd_ready").catch(() => {});

      return () => {
        unlistenLock();
        unlistenToast();
        unlistenThemeColor();
      };
    };

    const unlisten = setup();

    return () => {
      window.clearTimeout(hideTimer.current);
      delete window.__KEYBOARD_LOCK_OSD_SHOW;
      unlisten.then((cleanup) => cleanup());
    };
  }, []);

  return (
    <main className="osd-stage" aria-live="polite">
      <div
        className={`osd-pill ${notice?.kind === "toast" ? "toast" : ""} ${
          visible && notice ? "is-visible" : ""
        }`}
      >
        {notice?.kind === "lock" && (
          <>
            <LockIcon icon={notice.payload.icon} enabled={notice.payload.enabled} />
            <div className="osd-copy">
              <span className="osd-key">{notice.payload.abbreviation}</span>
              <span
                className={
                  notice.payload.enabled ? "osd-state on" : "osd-state off"
                }
              >
                {notice.payload.enabled ? "ON" : "OFF"}
              </span>
            </div>
          </>
        )}
        {notice?.kind === "toast" && (
          <div className="osd-toast-copy">
            <strong>{notice.payload.title}</strong>
            <span>{notice.payload.message}</span>
          </div>
        )}
      </div>
    </main>
  );
}

function SettingsView() {
  const [language, setLanguage] = useState<Language>(() => detectBrowserLanguage());
  const [states, setStates] = useState<LockChangePayload[]>(fallbackStates);
  const [autostartEnabled, setAutostartEnabled] = useState<boolean | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [suppressFullscreenOsd, setSuppressFullscreenOsd] = useState<boolean>(() =>
    readStoredBoolean(suppressFullscreenStorageKey, true),
  );
  const [osdEnabled, setOsdEnabled] = useState<OsdEnabledMap>(() =>
    readStoredOsdEnabled(),
  );
  const [osdPosition, setOsdPosition] = useState<OsdPositionId>(() =>
    readStoredOsdPosition(),
  );
  const [themeColor, setThemeColor] = useState(() => readStoredThemeColor());
  const [positionPickerOpen, setPositionPickerOpen] = useState(false);
  const [themePickerOpen, setThemePickerOpen] = useState(false);
  const positionPickerRef = useRef<HTMLDivElement>(null);
  const themePickerRef = useRef<HTMLDivElement>(null);
  const text = copy[language];

  useEffect(() => {
    invoke<LockChangePayload[]>("current_lock_states")
      .then(setStates)
      .catch(() => setStates(fallbackStates));
  }, []);

  useEffect(() => {
    const unlisten = listen<LockChangePayload>("lock-state-change", (event) => {
      setStates((current) =>
        current.map((state) =>
          state.key === event.payload.key ? event.payload : state,
        ),
      );
    });

    return () => {
      unlisten.then((cleanup) => cleanup());
    };
  }, []);

  useEffect(() => {
    invoke<boolean>("current_autostart_enabled")
      .then(setAutostartEnabled)
      .catch(() => setAutostartEnabled(true));
  }, []);

  useEffect(() => {
    invoke<string>("current_language")
      .then((value) => {
        if (isLanguage(value)) {
          setLanguage(value);
        }
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    invoke<string>("current_version")
      .then(setAppVersion)
      .catch(() => {});
  }, []);

  useEffect(() => {
    persistOsdEnabled(osdEnabled);
    Object.entries(osdEnabled).forEach(([key, enabled]) => {
      void invoke("set_osd_enabled", { key, enabled });
    });
  }, [osdEnabled]);

  useEffect(() => {
    persistBoolean(suppressFullscreenStorageKey, suppressFullscreenOsd);
    void invoke("set_suppress_fullscreen_osd", {
      enabled: suppressFullscreenOsd,
    });
  }, [suppressFullscreenOsd]);

  useEffect(() => {
    window.localStorage.setItem(osdPositionStorageKey, osdPosition);
    void invoke("set_osd_position", { position: osdPosition });
  }, [osdPosition]);

  useEffect(() => {
    window.localStorage.setItem(themeColorStorageKey, themeColor);
    applyThemeColor(themeColor);
    void invoke("set_osd_theme_color", { color: themeColor });
  }, [themeColor]);

  useEffect(() => {
    if (!positionPickerOpen && !themePickerOpen) {
      return;
    }

    const closePicker = (event: PointerEvent) => {
      const target = event.target as Node;
      if (
        !positionPickerRef.current?.contains(target) &&
        !themePickerRef.current?.contains(target)
      ) {
        setPositionPickerOpen(false);
        setThemePickerOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setPositionPickerOpen(false);
        setThemePickerOpen(false);
      }
    };

    document.addEventListener("pointerdown", closePicker);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closePicker);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [positionPickerOpen, themePickerOpen]);

  const enabledCount = useMemo(
    () => states.filter((state) => state.enabled).length,
    [states],
  );

  const preview = (state: LockChangePayload) => {
    void invoke("preview_osd", {
      key: state.key,
      enabled: !state.enabled,
    });
  };

  const toggleOsdEnabled = (key: LockKeyId, enabled: boolean) => {
    setOsdEnabled((current) => ({ ...current, [key]: enabled }));
  };

  const toggleAutostart = (enabled: boolean) => {
    const previous = autostartEnabled ?? true;
    setAutostartEnabled(enabled);
    void invoke<boolean>("set_autostart_enabled", { enabled })
      .then(setAutostartEnabled)
      .catch(() => setAutostartEnabled(previous));
  };

  return (
    <main className="settings-shell">
      <header className="settings-header">
        <div className="brand-lockup">
          <img alt="" className="brand-logo" src={logoUrl} />
          <div>
            <h1>{text.title}</h1>
            <p>{text.subtitle}</p>
          </div>
        </div>
        {appVersion && <span className="version-badge">v{appVersion}</span>}
      </header>

      <section className="settings-options">
        <label className="option-toggle">
          <input
            checked={autostartEnabled ?? true}
            type="checkbox"
            onChange={(event) => toggleAutostart(event.currentTarget.checked)}
          />
          <span>{text.startAtLogin}</span>
          <strong>{autostartEnabled ?? true ? text.on : text.off}</strong>
        </label>
        <label className="option-toggle">
          <input
            checked={suppressFullscreenOsd}
            type="checkbox"
            onChange={(event) =>
              setSuppressFullscreenOsd(event.currentTarget.checked)
            }
          />
          <span>{text.hideInFullscreen}</span>
          <strong>{suppressFullscreenOsd ? text.on : text.off}</strong>
        </label>
      </section>

      <section className="status-band">
        <div>
          <span>{text.status}</span>
          <strong>
            {enabledCount}/{states.length}
          </strong>
        </div>
        <div className="position-setting" ref={positionPickerRef}>
          <span>{text.position}</span>
          <button
            aria-expanded={positionPickerOpen}
            aria-haspopup="listbox"
            className="position-trigger"
            type="button"
            onClick={() => {
              setPositionPickerOpen((current) => !current);
              setThemePickerOpen(false);
            }}
          >
            <span className={`position-preview ${osdPosition}`} aria-hidden="true">
              <i />
            </span>
            <strong>{text.positions[osdPosition]}</strong>
            <span className="position-chevron" aria-hidden="true" />
          </button>
          {positionPickerOpen && (
            <div
              aria-label={text.selectPosition}
              className="position-picker"
              role="listbox"
            >
              <span className="position-picker-title">{text.selectPosition}</span>
              <div className="position-picker-grid">
                {osdPositionLayout.map((position) => (
                  <button
                    aria-label={text.positions[position]}
                    aria-selected={position === osdPosition}
                    className={`position-option ${
                      position === osdPosition ? "selected" : ""
                    }`}
                    key={position}
                    role="option"
                    title={text.positions[position]}
                    type="button"
                    onClick={() => {
                      setOsdPosition(position);
                      setPositionPickerOpen(false);
                    }}
                  >
                    <i aria-hidden="true" />
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>
        <div className="theme-setting" ref={themePickerRef}>
          <span>{text.themeColor}</span>
          <button
            aria-expanded={themePickerOpen}
            aria-haspopup="dialog"
            className="theme-trigger"
            type="button"
            onClick={() => {
              setThemePickerOpen((current) => !current);
              setPositionPickerOpen(false);
            }}
          >
            <span
              aria-hidden="true"
              className="theme-swatch"
              style={{ backgroundColor: themeColor }}
            />
            <strong>{themeColor.toUpperCase()}</strong>
            <span className="theme-chevron" aria-hidden="true" />
          </button>
          {themePickerOpen && (
            <div
              aria-label={text.selectThemeColor}
              className="theme-picker"
              role="dialog"
            >
              <span className="theme-picker-title">{text.selectThemeColor}</span>
              <div className="theme-preset-grid">
                {themeColorPresets.map((color) => (
                  <button
                    aria-label={color}
                    aria-pressed={color === themeColor}
                    className={`theme-preset ${
                      color === themeColor ? "selected" : ""
                    }`}
                    key={color}
                    style={{ backgroundColor: color }}
                    type="button"
                    onClick={() => setThemeColor(color)}
                  />
                ))}
              </div>
              <label className="theme-custom-color">
                <span>{text.customColor}</span>
                <input
                  aria-label={text.customColor}
                  type="color"
                  value={themeColor}
                  onChange={(event) => setThemeColor(event.currentTarget.value)}
                />
              </label>
            </div>
          )}
        </div>
      </section>

      <section className="settings-grid" aria-label={text.osd}>
        {states.map((state) => (
          <article className="key-row" key={state.key}>
            <LockIcon icon={state.icon} enabled={state.enabled} />
            <div className="key-copy">
              <span>{state.abbreviation}</span>
              <strong>{state.name}</strong>
            </div>
            <span className={state.enabled ? "state-chip on" : "state-chip off"}>
              {state.enabled ? text.on : text.off}
            </span>
            <label className="toggle">
              <input
                checked={osdEnabled[state.key]}
                type="checkbox"
                onChange={(event) =>
                  toggleOsdEnabled(state.key, event.currentTarget.checked)
                }
              />
              <span>{text.show}</span>
            </label>
            <button className="preview-button" type="button" onClick={() => preview(state)}>
              {text.preview}
            </button>
          </article>
        ))}
      </section>
    </main>
  );
}

function LockIcon({
  icon,
  enabled,
}: {
  icon: LockKeyId;
  enabled: boolean;
}) {
  return (
    <span className={`lock-icon ${icon} ${enabled ? "enabled" : "disabled"}`}>
      {icon === "caps" && <span>{enabled ? "ABC" : "abc"}</span>}
      {icon === "num" && <span>123</span>}
      {icon === "scroll" && (
        <span className="scroll-lines">
          <i />
          <i />
          <i />
        </span>
      )}
    </span>
  );
}

function readStoredOsdEnabled(): OsdEnabledMap {
  try {
    const value = window.localStorage.getItem(osdEnabledStorageKey);
    if (!value) {
      return defaultOsdEnabled;
    }

    const parsed = JSON.parse(value) as Partial<OsdEnabledMap>;
    return {
      caps: typeof parsed.caps === "boolean" ? parsed.caps : true,
      num: typeof parsed.num === "boolean" ? parsed.num : true,
      scroll: typeof parsed.scroll === "boolean" ? parsed.scroll : true,
    };
  } catch {
    return defaultOsdEnabled;
  }
}

function persistOsdEnabled(settings: OsdEnabledMap) {
  window.localStorage.setItem(osdEnabledStorageKey, JSON.stringify(settings));
}

function readStoredOsdPosition(): OsdPositionId {
  const value = window.localStorage.getItem(osdPositionStorageKey);
  return osdPositions.includes(value as OsdPositionId)
    ? (value as OsdPositionId)
    : defaultOsdPosition;
}

function readStoredThemeColor() {
  const value = window.localStorage.getItem(themeColorStorageKey);
  return isThemeColor(value) ? value : defaultThemeColor;
}

function applyThemeColor(color: string) {
  document.documentElement.style.setProperty("--accent-color", color);
}

function isThemeColor(value: string | null): value is string {
  return /^#[\da-f]{6}$/i.test(value ?? "");
}

function readStoredBoolean(key: string, fallback: boolean) {
  try {
    const value = window.localStorage.getItem(key);
    return value === null ? fallback : JSON.parse(value) === true;
  } catch {
    return fallback;
  }
}

function persistBoolean(key: string, value: boolean) {
  window.localStorage.setItem(key, JSON.stringify(value));
}

function isLanguage(value: string): value is Language {
  return value === "en" || value === "zh";
}

function detectBrowserLanguage(): Language {
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

export default App;
