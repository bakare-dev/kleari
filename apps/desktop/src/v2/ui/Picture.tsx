import { useEffect, useRef, useState, type ReactNode } from "react";
import { getPicture, type PictureSource } from "../../lib/pictureIpc";

/** Pictures asked for at once, at most, so a long list never floods the Mac. */
const AT_ONCE = 3;
/** Pictures remembered while Kleari is open, at most (oldest forgotten first). */
const REMEMBERED = 600;

const cache = new Map<string, string | null>();
const waiting: (() => void)[] = [];
let running = 0;

function next() {
  while (running < AT_ONCE && waiting.length > 0) {
    running += 1;
    waiting.shift()?.();
  }
}

/** Loads a picture through the queue, remembering it. */
function load(source: PictureSource, id: string, size: number): Promise<string | null> {
  const key = `${source}:${id}:${size}`;
  if (cache.has(key)) {
    return Promise.resolve(cache.get(key) ?? null);
  }
  return new Promise((resolve) => {
    waiting.push(() => {
      getPicture(source, id, size)
        .catch(() => null)
        .then((picture) => {
          cache.set(key, picture);
          if (cache.size > REMEMBERED) {
            const oldest = cache.keys().next().value;
            if (oldest !== undefined) {
              cache.delete(oldest);
            }
          }
          running -= 1;
          next();
          resolve(picture);
        });
    });
    next();
  });
}

/** A picture's URL once it is near the screen; `null` until then, or when there is none. */
export function usePicture(source: PictureSource, id: string, size: number, enabled = true) {
  const ref = useRef<HTMLSpanElement>(null);
  const [url, setUrl] = useState<string | null>(() => cache.get(`${source}:${id}:${size}`) ?? null);
  useEffect(() => {
    const element = ref.current;
    if (!enabled || element === null) {
      return;
    }
    let live = true;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          observer.disconnect();
          void load(source, id, size).then((picture) => live && setUrl(picture));
        }
      },
      { rootMargin: "200px" },
    );
    observer.observe(element);
    return () => {
      live = false;
      observer.disconnect();
    };
  }, [source, id, size, enabled]);
  return { ref, url };
}

/**
 * A row's picture (an image's thumbnail or an app's icon), loaded only once near the screen,
 * a few at a time; `fallback` (its usual icon tile) shows until then and when there is none.
 * With `onOpen` it is a button that opens the large preview.
 */
export function Picture({ source, id, size, label, fallback, enabled = true, onOpen, className }: { source: PictureSource; id: string; size: number; label: string; fallback: ReactNode; enabled?: boolean; onOpen?: () => void; className?: string }) {
  // Twice the shown size, for Retina screens.
  const { ref, url } = usePicture(source, id, size * 2, enabled);
  const image = url ? <img src={url} alt="" width={size} height={size} className="v2-picture-img" /> : fallback;
  return (
    <span ref={ref} className={`v2-picture${className ? ` ${className}` : ""}`} style={{ width: size, height: size }}>
      {onOpen && url ? (
        <button type="button" className="v2-picture-open" aria-label={`Preview ${label}`} onClick={onOpen}>
          {image}
        </button>
      ) : (
        image
      )}
    </span>
  );
}
