import {
  Children,
  cloneElement,
  createContext,
  isValidElement,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type HTMLAttributes,
  type ReactNode,
  type RefObject,
} from "react";

type Direction = "forward" | "back" | "lateral";
const Policy = createContext({ enabled: true, eco: false });
const ActiveRoute = createContext(true);
const Presence = createContext<{
  closing: boolean;
  complete: () => void;
} | null>(null);
export const MotionPolicy = Policy.Provider;
export function useRouteActive() {
  return useContext(ActiveRoute);
}
const aliases: Record<string, string> = {
  launch: "home",
  download: "download/game",
  mods: "download/mod",
};
export function readRoute() {
  const raw = location.hash.slice(1);
  return aliases[raw] || raw || "home";
}
function parent(route: string) {
  if (route.startsWith("game/")) return "download/game";
  if (route.startsWith("resource/"))
    return `download/${route.split("/")[1]?.split(":")[0] || "mod"}`;
  return route === "versions" ? "home" : "";
}
const historyKey = "astralV2Navigation";
export function useMotionRouter(beforeChange: (from: string) => void) {
  const [state, setState] = useState(() => ({
    route: readRoute(),
    direction: "lateral" as Direction,
    revision: 0,
  }));
  const current = useRef(state.route);
  const index = useRef(Number(history.state?.[historyKey]) || 0);
  const records = useRef(new Map([[index.current, state.route]]));
  const before = useRef(beforeChange);
  before.current = beforeChange;
  function apply(route: string, direction: Direction) {
    if (route === current.current) return;
    before.current(current.current);
    current.current = route;
    setState((old) => ({ route, direction, revision: old.revision + 1 }));
  }
  function navigate(path: string) {
    const route = aliases[path] || path;
    if (route === current.current) return;
    const direction = parent(current.current) === route ? "back" : "forward";
    index.current += 1;
    records.current.set(index.current, route);
    history.pushState(
      { ...history.state, [historyKey]: index.current },
      "",
      `#${route}`,
    );
    apply(route, direction);
  }
  function back(fallback: string) {
    if (records.current.get(index.current - 1) === fallback) history.back();
    else {
      history.replaceState(
        { ...history.state, [historyKey]: index.current },
        "",
        `#${fallback}`,
      );
      records.current.set(index.current, fallback);
      apply(fallback, "back");
    }
  }
  useEffect(() => {
    history.replaceState({ ...history.state, [historyKey]: index.current }, "");
    function changed() {
      const route = readRoute();
      if (route === current.current) return;
      const next =
        typeof history.state?.[historyKey] === "number"
          ? history.state[historyKey]
          : index.current + 1;
      const direction =
        next < index.current || parent(current.current) === route
          ? "back"
          : "forward";
      index.current = next;
      records.current.set(next, route);
      apply(route, direction);
    }
    window.addEventListener("popstate", changed);
    window.addEventListener("hashchange", changed);
    return () => {
      window.removeEventListener("popstate", changed);
      window.removeEventListener("hashchange", changed);
    };
  }, []);
  return { ...state, navigate, back };
}

function duration(element: Element, token: string, fallback: number) {
  const raw = getComputedStyle(element).getPropertyValue(token).trim();
  const number = Number.parseFloat(raw);
  return Number.isFinite(number)
    ? number * (raw.endsWith("ms") ? 1 : 1000)
    : fallback;
}
function tween(element: HTMLElement, frames: Keyframe[], milliseconds: number) {
  return element.animate(frames, {
    duration: milliseconds,
    easing:
      getComputedStyle(element).getPropertyValue("--motion-ease").trim() ||
      "cubic-bezier(.16,1,.3,1)",
    fill: "none",
  });
}

/** Visited panels retain drafts and in-flight operations. Animation never drives navigation or IPC. */
export function RouteDeck({
  route,
  direction,
  revision,
  main,
  scroll,
  intro,
  render,
}: {
  route: string;
  direction: Direction;
  revision: number;
  main: RefObject<HTMLElement | null>;
  scroll: Map<string, number>;
  intro: boolean;
  render: (route: string) => ReactNode;
}) {
  const { enabled, eco } = useContext(Policy);
  const root = useRef<HTMLDivElement>(null);
  const [visited, setVisited] = useState([route]);
  const routes = visited.includes(route) ? visited : [...visited, route];
  useEffect(() => {
    setVisited((old) => (old.includes(route) ? old : [...old, route]));
  }, [route]);
  const generation = useRef(0);
  const lastRevision = useRef(-1);
  const completedRevision = useRef(-1);
  useLayoutEffect(() => {
    const panel = root.current?.querySelector<HTMLElement>(
      ".route-panel:not([hidden])",
    );
    if (!panel) return;
    const ticket = ++generation.current;
    const first = revision === 0;
    const changed =
      lastRevision.current !== revision ||
      completedRevision.current !== revision;
    lastRevision.current = revision;
    if (!changed) return;
    main.current?.scrollTo(0, scroll.get(route) || 0);
    const focusBefore = document.activeElement;
    const focus = () => {
      if (
        generation.current !== ticket ||
        document.querySelector("dialog[open]")
      )
        return;
      const active = document.activeElement;
      if (
        active?.closest(".route-panel:not([hidden])") &&
        active.matches("button,input,textarea,select,[contenteditable=true]")
      )
        return;
      if (
        active === focusBefore ||
        active === document.body ||
        !active?.isConnected
      )
        panel.querySelector<HTMLElement>("h1")?.focus({ preventScroll: true });
    };
    const targets = panel.querySelector(".installer-page")
      ? [
          ...panel.querySelectorAll<HTMLElement>(
            ":scope > .installer-page > :not(.install-footer)",
          ),
        ]
      : [panel];
    const animations: Animation[] = [];
    if (enabled && (!first || intro) && typeof panel.animate === "function") {
      panel.dataset.transition = "entering";
      panel.dataset.direction = direction;
      const distance =
        Number.parseFloat(
          getComputedStyle(panel).getPropertyValue("--motion-distance"),
        ) || 6;
      const offset = eco
        ? Math.min(distance, 2)
        : direction === "back"
          ? -distance
          : distance;
      targets.forEach((target, i) => {
        const animation = tween(
          target,
          [
            { opacity: 0.62, transform: `translateY(${offset}px)` },
            { opacity: 1, transform: "translateY(0)" },
          ],
          duration(panel, "--motion-page", 240),
        );
        // A small content rhythm; no delayed mounting and no animated fixed install footer.
        if (targets.length > 1 && i > 1 && !eco)
          animation.effect?.updateTiming({ delay: 18 });
        animations.push(animation);
      });
      void Promise.all(
        animations.map((animation) => animation.finished.catch(() => {})),
      ).then(() => {
        if (generation.current === ticket) {
          completedRevision.current = revision;
          panel.dataset.transition = "idle";
          focus();
        }
      });
    } else {
      completedRevision.current = revision;
      panel.dataset.transition = "idle";
      focus();
    }
    return () => {
      ++generation.current;
      animations.forEach((animation) => animation.cancel());
      panel.dataset.transition = "idle";
    };
  }, [route, revision, enabled, eco]);
  return (
    <div className="route-deck" ref={root}>
      {routes.map((key) => (
        <ActiveRoute.Provider key={key} value={key === route}>
          <div
            className="route-panel"
            data-route={key}
            hidden={key !== route}
            inert={key !== route}
          >
            {render(key)}
          </div>
        </ActiveRoute.Provider>
      ))}
    </div>
  );
}

/** Animate local content without replacing it or restarting its data effects. */
export function MotionRegion({
  changeKey,
  children,
  className = "",
}: {
  changeKey: string;
  children: ReactNode;
  className?: string;
}) {
  const element = useRef<HTMLDivElement>(null);
  const previous = useRef(changeKey);
  const active = useRouteActive();
  const { enabled, eco } = useContext(Policy);
  useLayoutEffect(() => {
    const changed = previous.current !== changeKey;
    previous.current = changeKey;
    if (
      !changed ||
      !active ||
      !enabled ||
      !element.current ||
      typeof element.current.animate !== "function"
    )
      return;
    const animation = tween(
      element.current,
      [
        { opacity: 0.72, transform: `translateY(${eco ? 0 : 4}px)` },
        { opacity: 1, transform: "translateY(0)" },
      ],
      duration(element.current, "--motion-normal", 220),
    );
    return () => animation.cancel();
  }, [changeKey, active, enabled, eco]);
  return (
    <div ref={element} className={`motion-region ${className}`}>
      {children}
    </div>
  );
}

/** Retains the same modal subtree during exit; false never immediately destroys a form. */
export function ModalPresence({
  present,
  children,
}: {
  present: boolean;
  children: ReactNode;
}) {
  const [retained, setRetained] = useState(present);
  const last = useRef(children);
  const live = useRef(present);
  live.current = present;
  useLayoutEffect(() => {
    if (present) {
      last.current = children;
      setRetained(true);
    }
  }, [present, children]);
  if (!present && !retained) return null;
  return (
    <Presence.Provider
      value={{
        closing: !present,
        complete: () => {
          if (!live.current) setRetained(false);
        },
      }}
    >
      {present ? children : last.current}
    </Presence.Provider>
  );
}

export function useDialogMotion(
  ref: RefObject<HTMLDialogElement | null>,
  close: () => void,
) {
  const presence = useContext(Presence);
  const active = useRouteActive();
  const { enabled, eco } = useContext(Policy);
  const closing = presence?.closing || false;
  const complete = useRef(presence?.complete);
  complete.current = presence?.complete;
  const generation = useRef(0);
  const closeRef = useRef(close);
  const previousPose = useRef<{ opacity: string; transform: string } | null>(
    null,
  );
  closeRef.current = close;
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const ticket = ++generation.current;
    if (!active) {
      element.close();
      closeRef.current();
      complete.current?.();
      return;
    }
    if (!element.open) element.showModal();
    const drawer = !!element.querySelector(".diagnostic-content");
    element.dataset.dialogState = closing ? "closing" : "opening";
    element.dataset.drawer = String(drawer);
    const finish = () => {
      if (generation.current !== ticket) return;
      element.dataset.dialogState = closing ? "closed" : "open";
      previousPose.current = null;
      if (closing) {
        element.close();
        if (
          document.activeElement === document.body ||
          document.activeElement?.closest("[hidden]")
        )
          document
            .querySelector<HTMLElement>(".route-panel:not([hidden]) h1")
            ?.focus({ preventScroll: true });
        complete.current?.();
      }
    };
    if (!enabled || typeof element.animate !== "function") {
      finish();
      return;
    }
    const shift = eco ? 2 : drawer ? 24 : 6;
    const pose = drawer
      ? `translateX(${shift}px)`
      : `translateY(${shift}px) scale(.99)`;
    const frames = closing
      ? [
          previousPose.current || { opacity: 1, transform: "none" },
          { opacity: 0, transform: pose },
        ]
      : [
          previousPose.current || { opacity: 0, transform: pose },
          { opacity: 1, transform: "none" },
        ];
    const animation = tween(
      element,
      frames,
      duration(element, "--motion-dialog", 200),
    );
    void animation.finished.then(finish, () => {});
    return () => {
      ++generation.current;
      const current = getComputedStyle(element);
      previousPose.current = {
        opacity: current.opacity,
        transform: current.transform,
      };
      animation.cancel();
    };
  }, [closing, active, enabled, eco]);
  useEffect(
    () => () => {
      ref.current?.close();
    },
    [],
  );
  return closing;
}

/** Only small disclosures animate height. Virtualized catalog groups use opacity instead. */
export function MotionDetails({
  children,
  open = false,
  ...props
}: HTMLAttributes<HTMLDetailsElement> & { open?: boolean }) {
  const element = useRef<HTMLDetailsElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(open);
  const wanted = useRef(open);
  const animation = useRef<Animation | null>(null);
  const { enabled } = useContext(Policy);
  const active = useRouteActive();
  const parts = Children.toArray(children);
  const summary = parts.shift();
  const settle = () => {
    if (element.current) element.current.open = wanted.current;
    if (body.current) {
      body.current.style.height = "";
      body.current.style.overflow = "";
    }
  };
  useEffect(() => {
    if (!enabled || !active) {
      animation.current?.cancel();
      settle();
    }
    return () => animation.current?.cancel();
  }, [enabled, active]);
  function toggle(event: React.MouseEvent) {
    event.preventDefault();
    const container = element.current;
    const content = body.current;
    if (!container || !content) return;
    const height = container.open ? content.getBoundingClientRect().height : 0;
    const opacity = container.open
      ? Number(getComputedStyle(content).opacity)
      : 0;
    animation.current?.cancel();
    wanted.current = !wanted.current;
    setExpanded(wanted.current);
    if (!enabled || !active || typeof content.animate !== "function") {
      settle();
      return;
    }
    container.open = true;
    content.style.overflow = "hidden";
    const next = content.animate(
      [
        { height: `${height}px`, opacity },
        {
          height: `${wanted.current ? content.scrollHeight : 0}px`,
          opacity: wanted.current ? 1 : 0,
        },
      ],
      {
        duration: duration(container, "--motion-normal", 220),
        easing: "cubic-bezier(.16,1,.3,1)",
        fill: "both",
      },
    );
    animation.current = next;
    void next.finished.then(
      () => {
        if (animation.current === next) {
          settle();
          next.cancel();
          animation.current = null;
        }
      },
      () => {},
    );
  }
  return (
    <details {...props} ref={element} open={open} data-expanded={expanded}>
      {isValidElement(summary)
        ? cloneElement(
            summary as React.ReactElement<HTMLAttributes<HTMLElement>>,
            { onClick: toggle, "aria-expanded": expanded },
          )
        : summary}
      <div className="disclosure-content" ref={body}>
        {parts}
      </div>
    </details>
  );
}
