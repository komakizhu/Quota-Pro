import { ArrowClockwise, ArrowDown, ArrowUp, ArrowsInSimple, ClockCounterClockwise, CloudSlash, Info, PushPin, PushPinSlash, SignIn, WarningCircle } from "@phosphor-icons/react";
import { memo, type CSSProperties, type MouseEvent as ReactMouseEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { clampPercent, formatDateTime, formatResetDate, formatResetTime, quotaTier } from "../lib/format";
import { blurProgressSegments } from "../lib/blurSkin";
import { copy, normalizeLanguage } from "../lib/i18n";
import { consumeOrbClick, createOrbDragState, recordOrbDrag } from "../lib/orbGesture";
import { COMPACT_SIZE_RANGE, EXPANDED_SIZE_RANGE, getResizeEdge, resizeHasMoved, resizeSizeFromPointer, type ResizeEdge } from "../lib/resize";
import type { Language, ProviderSnapshot, ToggleCorner, WidgetPreferences, WidgetSkin, WidgetTheme } from "../types";
import { ProviderMark } from "./ProviderMark";
import computerGptLogoUrl from "../../assets/computer-gpt-logo.svg";
import computerOrbBaseUrl from "../../assets/computer-orb-base.svg";
import computerOrbHealthyUrl from "../../assets/computer-orb-screen-healthy.svg";
import computerOrbCautionUrl from "../../assets/computer-orb-screen-caution.svg";
import computerOrbCriticalUrl from "../../assets/computer-orb-screen-critical.svg";
import computerErrorUnavailableUrl from "../../assets/computer-error-unavailable.svg";
import computerErrorStaleUrl from "../../assets/computer-error-stale.svg";
import computerErrorSignedOutUrl from "../../assets/computer-error-signedout.svg";
import computerOrbErrorScreenUrl from "../../assets/computer-orb-screen-error.svg";
import computerOrbGptUrl from "../../assets/computer-orb-gpt.svg";

interface Props {
  snapshot: ProviderSnapshot;
  preferences: WidgetPreferences;
  providerCount: number;
  onPrevious: () => void;
  onNext: () => void;
  onTogglePin: () => void;
  onLock: () => void;
  onCollapse: () => void;
  toggleCorner: ToggleCorner;
  onDrag: () => void | Promise<void>;
  onResizeStart?: (edge: ResizeEdge) => Promise<void>;
  onResizePreview?: (size: number) => void;
  onResizeCommit?: (size: number) => Promise<void>;
  onResizeCancel?: () => Promise<void>;
  onResizeReset?: () => Promise<void>;
  resizeSize?: number;
  onRefresh?: () => void;
  isConsuming?: boolean;
  notice?: ReactNode;
  initialShowCreditTip?: boolean;
  theme?: WidgetTheme;
  skin?: WidgetSkin;
  style?: CSSProperties;
}

function StatusIcon({ status, expired = false }: { status: ProviderSnapshot["status"]; expired?: boolean }) {
  if (status === "signed_out") return <SignIn weight="duotone" />;
  if (status === "stale" || expired) return <ClockCounterClockwise weight="duotone" />;
  if (status === "unavailable") return <CloudSlash weight="duotone" />;
  return <WarningCircle weight="duotone" />;
}

function ComputerErrorArtwork({ status }: { status: ProviderSnapshot["status"] }) {
  const src = status === "signed_out"
    ? computerErrorSignedOutUrl
    : status === "stale"
      ? computerErrorStaleUrl
      : computerErrorUnavailableUrl;
  return <img className={`computer-error-artwork computer-error-artwork--${status}`} src={src} alt="" />;
}

function localizedBackendMessage(message: string | null, language: Language): string | null {
  if (!message) return null;
  if (language === "en") return message;
  const normalized = message.toLowerCase();
  if (normalized.includes("sign in") || normalized.includes("login")) return "Codex 登录已失效，请重新登录。";
  if (normalized.includes("rate limited")) return "请求过于频繁，将稍后自动重试。";
  if (normalized.includes("network")) return "网络不可用，将自动重试。";
  if (normalized.includes("format")) return "额度响应格式已变化。";
  if (normalized.includes("missing the 5h")) return "额度响应缺少 5 小时窗口。";
  if (normalized.includes("refresh is already running")) return "额度正在刷新，请稍候。";
  return message;
}

function BlurProgress({ percent, label }: { percent: number; label: string }) {
  const segments = blurProgressSegments(percent);
  const availableCount = segments.filter(Boolean).length;
  return <div className="blur-progress" role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
    {segments.map((available, index) => {
      const endWeight = available && availableCount > 1 ? (index / (availableCount - 1)) * 100 : 0;
      return <i key={index} className={available ? "is-available" : "is-used"} style={available ? { "--blur-progress-end-weight": `${endWeight}%` } as CSSProperties : undefined} aria-hidden="true" />;
    })}
  </div>;
}

function ComputerProgress({ percent, label }: { percent: number; label: string }) {
  const segments = 34;
  const available = Math.round((Math.max(0, Math.min(100, percent)) / 100) * segments);
  return <div className="computer-progress" role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
    {Array.from({ length: segments }, (_, index) => {
      const endWeight = index < available && available > 1 ? (index / (available - 1)) * 100 : 0;
      return <i key={index} className={index < available ? "is-available" : "is-used"} style={index < available ? { "--computer-progress-end-weight": `${endWeight}%` } as CSSProperties : undefined} aria-hidden="true" />;
    })}
  </div>;
}

export const QuotaCard = memo(function QuotaCard({
  snapshot,
  preferences,
  providerCount,
  onPrevious,
  onNext,
  onTogglePin: _onTogglePin,
  onLock,
  onCollapse,
  toggleCorner,
  onDrag,
  onResizeStart,
  onResizePreview,
  onResizeCommit,
  onResizeCancel,
  onResizeReset,
  resizeSize = 306,
  onRefresh,
  isConsuming = false,
  notice = null,
  initialShowCreditTip = false,
  theme,
  skin = "default",
  style,
}: Props) {
  const [showCreditTip, setShowCreditTip] = useState(initialShowCreditTip);
  const [hoveredResizeEdge, setHoveredResizeEdge] = useState<ResizeEdge | null>(null);
  const [activeResizeEdge, setActiveResizeEdge] = useState<ResizeEdge | null>(null);
  const [previewSize, setPreviewSize] = useState(resizeSize);
  const resizeCleanup = useRef<(() => void) | null>(null);
  useEffect(() => {
    if (!activeResizeEdge) setPreviewSize(resizeSize);
  }, [activeResizeEdge, resizeSize]);
  useEffect(() => () => resizeCleanup.current?.(), []);
  const language = normalizeLanguage(preferences.language);
  const t = copy[language];
  const primary = snapshot.shortWindow ? clampPercent(snapshot.shortWindow.remainingPercent) : null;
  const weekly = snapshot.weeklyWindow ? clampPercent(snapshot.weeklyWindow.remainingPercent) : null;
  const displayPercent = primary ?? weekly;
  const displayWindow = snapshot.shortWindow ?? snapshot.weeklyWindow;
  const displayingWeeklyAsPrimary = primary === null && weekly !== null;
  const staleAge = Date.now() - new Date(snapshot.updatedAt).getTime();
  const staleExpired = snapshot.status === "stale" && staleAge > 30 * 60_000;
  const available = snapshot.status === "ok" || (snapshot.status === "stale" && !staleExpired);
  const tier = quotaTier(displayPercent);
  const indicatorState = isConsuming ? "active" : snapshot.status === "ok" ? "ok" : snapshot.status === "stale" ? "stale" : "error";
  const indicatorLabel = isConsuming
    ? t.active
    : snapshot.status === "ok"
      ? t.dataSynced
      : snapshot.status === "stale"
        ? t.dataStale
        : snapshot.status === "signed_out"
          ? t.notSignedIn
          : t.unavailableStatus;
  const message = localizedBackendMessage(snapshot.message, language);
  const creditExpirations = useMemo(() => (snapshot.resetCreditExpiresAt ?? []).map((value, index) => {
    return t.creditItem(index, formatDateTime(value, language));
  }), [language, snapshot.resetCreditExpiresAt, t]);

  const resizeClass = activeResizeEdge ?? hoveredResizeEdge;
  const resizeStyle = { ...style, "--widget-scale": String(previewSize / 306) } as CSSProperties;
  const isExcludedResizeTarget = (target: EventTarget | null) => target instanceof Element && Boolean(target.closest("button, a, input, textarea, select, nav"));
  const startResize = (event: ReactMouseEvent<HTMLElement>): boolean => {
    if (event.button !== 0 || isExcludedResizeTarget(event.target)) return false;
    const edge = getResizeEdge(event.clientX, event.clientY, event.currentTarget.getBoundingClientRect());
    if (!edge) return false;
    event.preventDefault();
    event.stopPropagation();
    const start = { x: event.clientX, y: event.clientY };
    const startSize = previewSize;
    setActiveResizeEdge(edge);
    let ready = false;
    let released = false;
    let moved = false;
    let finished = false;
    let latestSize = startSize;
    const commit = () => {
      if (finished) return;
      finished = true;
      resizeCleanup.current?.();
      resizeCleanup.current = null;
      void Promise.resolve(onResizeCommit?.(latestSize)).catch(() => {
        setPreviewSize(startSize);
      }).finally(() => {
        setActiveResizeEdge(null);
        setHoveredResizeEdge(null);
      });
    };
    const cancel = () => {
      if (finished) return;
      finished = true;
      resizeCleanup.current?.();
      resizeCleanup.current = null;
      void Promise.resolve(onResizeCancel?.()).finally(() => {
        setPreviewSize(startSize);
        setActiveResizeEdge(null);
        setHoveredResizeEdge(null);
      });
    };
    const onMove = (move: MouseEvent) => {
      if (!moved && !resizeHasMoved(start.x, start.y, move.clientX, move.clientY)) return;
      moved = true;
      latestSize = resizeSizeFromPointer(startSize, edge, move.clientX - start.x, move.clientY - start.y, EXPANDED_SIZE_RANGE);
      setPreviewSize(latestSize);
      if (ready) onResizePreview?.(latestSize);
    };
    const onUp = () => {
      released = true;
      if (ready) (moved ? commit : cancel)();
    };
    const onKeyDown = (keyboardEvent: KeyboardEvent) => { if (keyboardEvent.key === "Escape") cancel(); };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp, { once: true });
    window.addEventListener("blur", cancel, { once: true });
    window.addEventListener("keydown", onKeyDown);
    resizeCleanup.current = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("blur", cancel);
      window.removeEventListener("keydown", onKeyDown);
    };
    void (async () => {
      try {
        await onResizeStart?.(edge);
      } catch {
        resizeCleanup.current?.();
        resizeCleanup.current = null;
        setActiveResizeEdge(null);
        setPreviewSize(startSize);
        return;
      }
      ready = true;
      if (latestSize !== startSize) onResizePreview?.(latestSize);
      if (released) (moved ? commit : cancel)();
    })();
    return true;
  };

  const resetFromResizeEdge = (event: ReactMouseEvent<HTMLElement>) => {
    if (isExcludedResizeTarget(event.target)) return;
    const edge = getResizeEdge(event.clientX, event.clientY, event.currentTarget.getBoundingClientRect());
    if (!edge || !onResizeReset) return;
    event.preventDefault();
    event.stopPropagation();
    const previousSize = previewSize;
    void onResizeReset()
      .then(() => setPreviewSize(306))
      .catch(() => setPreviewSize(previousSize));
  };

  return (
    <main
      className={`quota-card quota-card--${snapshot.status} quota-card--${tier}${theme ? ` quota-card--theme-${theme}` : ""}${skin === "blur" ? " quota-card--skin-blur" : ""}${skin === "computer" ? " quota-card--skin-computer" : ""}${resizeClass ? ` quota-resize--${resizeClass}` : ""}${activeResizeEdge ? " is-resizing" : ""}`}
      style={resizeStyle}
      onMouseMove={(event) => { if (!activeResizeEdge) setHoveredResizeEdge(getResizeEdge(event.clientX, event.clientY, event.currentTarget.getBoundingClientRect())); }}
      onMouseLeave={() => { if (!activeResizeEdge) setHoveredResizeEdge(null); }}
      onMouseDownCapture={startResize}
      onDoubleClick={resetFromResizeEdge}
    >
      <div className="aurora" aria-hidden="true" />
      <span className="sr-only">拖动卡片边缘或角落可调整大小</span>
      <span className="sr-only" aria-live="polite">{available && displayPercent !== null ? (displayingWeeklyAsPrimary ? t.weeklyAvailableLabel(displayPercent) : t.availableLabel(displayPercent)) : message}</span>
      {notice ? <div className="operation-notice" role="status">{notice}</div> : null}
      <header className="card-header" onMouseDown={(event) => { if (event.button === 0 && !getResizeEdge(event.clientX, event.clientY, event.currentTarget.parentElement?.getBoundingClientRect() ?? event.currentTarget.getBoundingClientRect()) && !isExcludedResizeTarget(event.target)) void onDrag(); }}>
        <div>
          <p className="eyebrow">{skin === "computer" ? "codex·plus" : `${snapshot.displayName} · ${snapshot.plan ?? t.accountFallback}`}</p>
          {snapshot.status !== "stale" ? <p className="updated">{displayingWeeklyAsPrimary ? t.weeklyShortRemaining : t.shortRemaining}</p> : null}
        </div>
        {!preferences.locked ? (
          <nav className="card-actions" aria-label={t.controls} onMouseDown={(event) => event.stopPropagation()}>
            {providerCount > 1 ? <button onClick={onPrevious} aria-label={t.servicePrevious}><ArrowUp /></button> : null}
            {providerCount > 1 ? <button onClick={onNext} aria-label={t.serviceNext}><ArrowDown /></button> : null}
            <span className={`usage-indicator usage-indicator--${indicatorState}`} role="status" aria-label={indicatorLabel} title={indicatorLabel}><i /></span>
            <button className={preferences.alwaysOnTop ? "pin-button pin-button--active" : "pin-button"} onClick={onLock} aria-pressed={preferences.alwaysOnTop} aria-label={preferences.alwaysOnTop ? t.pinOff : t.pinOn} title={preferences.alwaysOnTop ? t.pinOff : t.pinOn}>
              {preferences.alwaysOnTop ? <PushPin weight="fill" /> : <PushPinSlash />}
            </button>
          </nav>
        ) : null}
      {!preferences.locked ? (
        <button className={`widget-toggle widget-toggle--${toggleCorner}`} onMouseDown={(event) => event.stopPropagation()} onClick={onCollapse} aria-label={t.collapseWidget} title={t.collapseWidget}>
          <ArrowsInSimple weight="bold" />
        </button>
      ) : null}
      </header>

      {available && displayPercent !== null ? (
        <>
          <section className="primary-metric" aria-label={displayingWeeklyAsPrimary ? t.weeklyAvailableLabel(displayPercent) : t.availableLabel(displayPercent)}>
            <span>{displayPercent}</span><small>%</small>
          </section>
          {skin === "blur"
            ? <BlurProgress percent={displayPercent} label={displayingWeeklyAsPrimary ? t.weeklyAvailableLabel(displayPercent) : t.availableLabel(displayPercent)} />
            : skin === "computer"
              ? <ComputerProgress percent={displayPercent} label={displayingWeeklyAsPrimary ? t.weeklyAvailableLabel(displayPercent) : t.availableLabel(displayPercent)} />
            : <div className="progress" role="progressbar" aria-label={displayingWeeklyAsPrimary ? t.weeklyAvailableLabel(displayPercent) : t.availableLabel(displayPercent)} aria-valuemin={0} aria-valuemax={100} aria-valuenow={displayPercent}><span style={{ width: `${displayPercent}%` }} /></div>}
          <p className="reset-time">{formatResetTime(displayWindow?.resetsAt ?? null, new Date(), language)}{displayWindow?.resetsAt ? ` · ${formatDateTime(displayWindow.resetsAt, language)}` : ""}</p>
          <footer className="card-footer">
            <div className="weekly-metric">
              {displayingWeeklyAsPrimary ? <p className="weekly-note"><Info weight="bold" aria-hidden="true" />{t.shortWindowUnavailable}</p> : <p>{t.weeklyUntil(formatResetDate(snapshot.weeklyWindow?.resetsAt ?? null, language))}</p>}
              <strong className={displayingWeeklyAsPrimary ? "weekly-value--unavailable" : undefined}>{displayingWeeklyAsPrimary ? "--" : weekly ?? "--"}<small>{displayingWeeklyAsPrimary || weekly === null ? "" : "%"}</small></strong>
              <div className="reset-credit-row" onMouseDown={(event) => event.stopPropagation()}>
                <span>{snapshot.resetCredits === null ? t.resetCreditUnknown : t.resetCredits(snapshot.resetCredits)}</span>
                {snapshot.resetCredits !== null && snapshot.resetCredits > 0 ? (
                  <button type="button" className="reset-credit-button" onClick={() => setShowCreditTip((value) => !value)} aria-expanded={showCreditTip} aria-label={t.view}>{t.view}</button>
                ) : null}
              </div>
              {showCreditTip ? (
                <div className="reset-credit-tip" role="status" onMouseDown={(event) => event.stopPropagation()}>
                  {creditExpirations.length > 0 ? creditExpirations.map((item) => <p key={item}>{item}</p>) : <p>{t.noCreditExpiration}</p>}
                </div>
              ) : null}
            </div>
            {skin === "blur" ? null : skin === "computer" ? <div className="computer-gpt-mark"><img src={computerGptLogoUrl} alt="GPT" /></div> : <ProviderMark />}
          </footer>
        </>
      ) : (
        <section className="error-state" aria-live="polite">
          {skin === "computer"
            ? <div className="status-icon status-icon--computer" aria-hidden="true"><ComputerErrorArtwork status={snapshot.status} /></div>
            : <div className="status-icon" aria-hidden="true"><StatusIcon status={snapshot.status} expired={staleExpired} /></div>}
          <strong>{snapshot.status === "signed_out" ? t.signedInRequired : staleExpired ? t.staleExpired : t.temporarilyUnavailable}</strong>
          <p>{message ?? t.errorUnavailable}</p>
          {snapshot.status === "stale" ? (
            <button type="button" className="error-refresh-button" onMouseDown={(event) => event.stopPropagation()} onClick={onRefresh} disabled={!onRefresh} aria-label={t.refreshQuota}>
              <ArrowClockwise />
              <span>{t.refresh}</span>
            </button>
          ) : null}
        </section>
      )}
    </main>
  );
});

export const QuotaOrb = memo(function QuotaOrb({ snapshot, onDrag, onExpand, onResizeStart, onResizePreview, onResizeCommit, onResizeCancel, onResizeReset, resizeSize = 72, language = "zh-CN", theme, skin = "default", style }: Pick<Props, "snapshot" | "onDrag" | "theme" | "skin" | "style" | "onResizeStart" | "onResizePreview" | "onResizeCommit" | "onResizeCancel" | "onResizeReset" | "resizeSize"> & { language?: Language; onExpand: () => void }) {
  const [idle, setIdle] = useState(false);
  const [hoveredResizeEdge, setHoveredResizeEdge] = useState<ResizeEdge | null>(null);
  const [activeResizeEdge, setActiveResizeEdge] = useState<ResizeEdge | null>(null);
  const [previewSize, setPreviewSize] = useState(resizeSize);
  const idleTimer = useRef<number | null>(null);
  const dragCleanup = useRef<(() => void) | null>(null);
  const resizeCleanup = useRef<(() => void) | null>(null);
  const dragClickState = useRef(createOrbDragState());
  const nativeDragActive = useRef(false);
  const dragCooldownUntil = useRef(0);
  const resizing = useRef(false);
  const activeLanguage = normalizeLanguage(language);
  const t = copy[activeLanguage];
  const primary = snapshot.shortWindow ? clampPercent(snapshot.shortWindow.remainingPercent) : null;
  const weekly = snapshot.weeklyWindow ? clampPercent(snapshot.weeklyWindow.remainingPercent) : null;
  const displayPercent = primary ?? weekly;
  const displayingWeeklyAsPrimary = primary === null && weekly !== null;
  const tier = quotaTier(displayPercent);
  const available = snapshot.status === "ok" && displayPercent !== null;
  const computerScreen = tier === "caution"
    ? computerOrbCautionUrl
    : tier === "critical"
      ? computerOrbCriticalUrl
      : computerOrbHealthyUrl;
  const computerOrbErrorSymbol = snapshot.status === "signed_out"
    ? computerOrbGptUrl
    : snapshot.status === "stale"
      ? computerErrorStaleUrl
      : computerErrorUnavailableUrl;

  useEffect(() => {
    idleTimer.current = window.setTimeout(() => setIdle(true), 2000);
    return () => {
      if (idleTimer.current !== null) window.clearTimeout(idleTimer.current);
      dragCleanup.current?.();
      resizeCleanup.current?.();
    };
  }, []);

  useEffect(() => {
    if (!activeResizeEdge) setPreviewSize(resizeSize);
  }, [activeResizeEdge, resizeSize]);

  const handleMouseEnter = () => {
    if (idleTimer.current !== null) window.clearTimeout(idleTimer.current);
    setIdle(false);
  };

  const handleMouseDown = (event: ReactMouseEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    // Native macOS dragging can briefly replay a mousedown before its release
    // click reaches WebKit. Keep the guard through that replay and through a
    // short post-release cooldown; otherwise that synthetic sequence can turn
    // a move into an expand action. If no click is produced, the guard is
    // cleared on the first gesture after the cooldown.
    const gestureProtected = nativeDragActive.current || resizing.current || Date.now() < dragCooldownUntil.current;
    if (gestureProtected) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    dragClickState.current = createOrbDragState();
    const edge = getResizeEdge(event.clientX, event.clientY, event.currentTarget.getBoundingClientRect());
    if (edge) {
      event.preventDefault();
      event.stopPropagation();
      dragClickState.current = recordOrbDrag(dragClickState.current);
      const start = { x: event.clientX, y: event.clientY };
      const startSize = previewSize;
      setActiveResizeEdge(edge);
      resizing.current = true;
      let ready = false;
      let released = false;
      let moved = false;
      let finished = false;
      let latestSize = startSize;
      const commit = () => {
        if (finished) return;
        finished = true;
        dragCooldownUntil.current = Math.max(dragCooldownUntil.current, Date.now() + 350);
        resizeCleanup.current?.();
        resizeCleanup.current = null;
        void Promise.resolve(onResizeCommit?.(latestSize)).catch(() => setPreviewSize(startSize)).finally(() => {
          resizing.current = false;
          setActiveResizeEdge(null);
          setHoveredResizeEdge(null);
        });
      };
      const cancel = () => {
        if (finished) return;
        finished = true;
        dragCooldownUntil.current = Math.max(dragCooldownUntil.current, Date.now() + 350);
        resizeCleanup.current?.();
        resizeCleanup.current = null;
        void Promise.resolve(onResizeCancel?.()).finally(() => {
          resizing.current = false;
          setPreviewSize(startSize);
          setActiveResizeEdge(null);
          setHoveredResizeEdge(null);
        });
      };
      const onMove = (move: MouseEvent) => {
        if (!moved && !resizeHasMoved(start.x, start.y, move.clientX, move.clientY)) return;
        moved = true;
        latestSize = resizeSizeFromPointer(startSize, edge, move.clientX - start.x, move.clientY - start.y, COMPACT_SIZE_RANGE);
        setPreviewSize(latestSize);
        if (ready) onResizePreview?.(latestSize);
      };
      const onUp = () => {
        released = true;
        if (ready) (moved ? commit : cancel)();
      };
      const onKeyDown = (keyboardEvent: KeyboardEvent) => { if (keyboardEvent.key === "Escape") cancel(); };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp, { once: true });
      window.addEventListener("blur", cancel, { once: true });
      window.addEventListener("keydown", onKeyDown);
      resizeCleanup.current = () => {
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
        window.removeEventListener("blur", cancel);
        window.removeEventListener("keydown", onKeyDown);
      };
      void (async () => {
        try {
          await onResizeStart?.(edge);
        } catch {
          resizeCleanup.current?.();
          resizeCleanup.current = null;
          resizing.current = false;
          dragCooldownUntil.current = Date.now() + 350;
          setActiveResizeEdge(null);
          setPreviewSize(startSize);
          return;
        }
        ready = true;
        if (latestSize !== startSize) onResizePreview?.(latestSize);
        if (released) (moved ? commit : cancel)();
      })();
      return;
    }
    const start = { x: event.clientX, y: event.clientY };
    let dragged = false;
    const onMove = (move: MouseEvent) => {
      if (Math.hypot(move.clientX - start.x, move.clientY - start.y) < 6) return;
      dragClickState.current = recordOrbDrag(dragClickState.current);
      dragged = true;
      nativeDragActive.current = true;
      // Keep the release listener alive while the native drag owns the
      // pointer. WebKit may not send the final click to the original target,
      // but it can still deliver this window-level mouseup; using it to start
      // the post-release cooldown closes that race.
      window.removeEventListener("mousemove", onMove);
      void Promise.resolve(onDrag()).catch(() => undefined).finally(() => {
        nativeDragActive.current = false;
        dragCooldownUntil.current = Date.now() + 350;
      });
    };
    const onUp = () => {
      if (dragged) {
        nativeDragActive.current = false;
        dragCooldownUntil.current = Math.max(dragCooldownUntil.current, Date.now() + 350);
      }
      dragCleanup.current?.();
      dragCleanup.current = null;
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp, { once: true });
    dragCleanup.current = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  };

  const resetFromResizeEdge = (event: ReactMouseEvent<HTMLElement>) => {
    const edge = getResizeEdge(event.clientX, event.clientY, event.currentTarget.getBoundingClientRect());
    if (!edge || !onResizeReset) return;
    event.preventDefault();
    event.stopPropagation();
    const previousSize = previewSize;
    void onResizeReset()
      .then(() => setPreviewSize(72))
      .catch(() => setPreviewSize(previousSize));
  };

  const handleClick = () => {
    const clickGuard = consumeOrbClick(dragClickState.current);
    dragClickState.current = clickGuard.state;
    const inDragCooldown = Date.now() < dragCooldownUntil.current;
    if (clickGuard.suppressed || resizing.current || inDragCooldown) return;
    onExpand();
  };

  return (
    <main
      style={{ ...style, "--widget-scale": String(previewSize / 72) } as CSSProperties}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={() => {
        if (idleTimer.current !== null) window.clearTimeout(idleTimer.current);
        idleTimer.current = window.setTimeout(() => setIdle(true), 2000);
        if (!activeResizeEdge) setHoveredResizeEdge(null);
      }}
      onMouseDown={handleMouseDown}
      onDoubleClick={resetFromResizeEdge}
      onMouseMove={(event) => { if (!activeResizeEdge) setHoveredResizeEdge(getResizeEdge(event.clientX, event.clientY, event.currentTarget.getBoundingClientRect())); }}
      onClick={handleClick}
      onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); onExpand(); } }}
      role="button"
      tabIndex={0}
      aria-label={available ? (displayingWeeklyAsPrimary ? t.weeklyAvailableLabel(displayPercent!) : t.availableLabel(displayPercent!)) : localizedBackendMessage(snapshot.message, activeLanguage) ?? t.unavailableStatus}
      className={`quota-orb quota-card--${snapshot.status} quota-card--${tier}${theme ? ` quota-orb--theme-${theme}` : ""}${skin === "blur" ? " quota-orb--skin-blur" : ""}${skin === "computer" ? " quota-orb--skin-computer" : ""}${displayingWeeklyAsPrimary ? " quota-orb--weekly" : ""}${idle ? " quota-orb--idle" : ""}${(activeResizeEdge ?? hoveredResizeEdge) ? ` quota-resize--${activeResizeEdge ?? hoveredResizeEdge}` : ""}${activeResizeEdge ? " is-resizing" : ""}`}
    >
      <div className="aurora" aria-hidden="true" />
      {skin === "computer" ? <img className="computer-orb-base" src={computerOrbBaseUrl} alt="" aria-hidden="true" /> : null}
      {skin === "computer" ? <img className="computer-orb-screen" src={available ? computerScreen : computerOrbErrorScreenUrl} alt="" aria-hidden="true" /> : null}
      {available && displayingWeeklyAsPrimary && skin !== "computer" ? (
        <span className="orb-weekly-badge" aria-hidden="true">
          <svg viewBox="0 0 55 17" fill="none" xmlns="http://www.w3.org/2000/svg">
            <path d="M7.3687 52.2894C13.0674 47.8486 17 38.4172 17 27.5C17 16.5828 13.0674 7.15141 7.3687 2.71063C3.88364 -0.00516105 0 3.58172 0 8L0 47C0 51.4183 3.88364 55.0052 7.3687 52.2894Z" fill="currentColor" transform="matrix(0 1 -1 0 55 0)" />
          </svg>
          <b>W</b>
        </span>
      ) : null}
      {available ? (
        <section className="orb-metric">
          <span>{displayPercent}</span>
          {skin !== "computer" ? <small>%</small> : null}
        </section>
      ) : (
        <section className="orb-unavailable">
          {skin === "computer"
            ? <img className={`computer-orb-error-symbol computer-orb-error-symbol--${snapshot.status}`} src={computerOrbErrorSymbol} alt="" aria-hidden="true" />
            : <StatusIcon status={snapshot.status} />}
        </section>
      )}
    </main>
  );
});
