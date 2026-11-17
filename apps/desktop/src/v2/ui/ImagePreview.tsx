import { useEffect, useState } from "react";
import type { PictureSource } from "../../lib/pictureIpc";
import { Button, Modal } from "./components";
import { Picture, usePicture } from "./Picture";
import { IconTile } from "./components";

/** One image the preview can show. */
export interface PreviewImage {
  id: string;
  name: string;
  /** A line under its name, such as its folder and size. */
  detail: string;
  /** "Keep", "Kept as is" and so on, for a group of copies or look-alikes. */
  note?: string;
}

/** The large picture, loaded when shown. */
function Large({ source, image }: { source: PictureSource; image: PreviewImage }) {
  const { ref, url } = usePicture(source, image.id, 1600);
  return (
    <span ref={ref} className="v2-preview-large">
      {url ? <img src={url} alt={image.name} /> : <span className="v2-card-note">Making the preview…</span>}
    </span>
  );
}

/**
 * Images seen large inside Kleari: the one chosen, and the others in its group in a strip
 * below to compare (← → move between them). Pictures are made on this Mac and never stored.
 */
export function ImagePreview({ source, images, start, onClose }: { source: PictureSource; images: readonly PreviewImage[]; start: number; onClose: () => void }) {
  const [at, setAt] = useState(start);
  const image = images[Math.min(at, images.length - 1)];

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "ArrowRight") {
        setAt((current) => Math.min(current + 1, images.length - 1));
      } else if (event.key === "ArrowLeft") {
        setAt((current) => Math.max(current - 1, 0));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [images.length]);

  if (image === undefined) {
    return null;
  }
  return (
    <Modal open onClose={onClose} labelledBy="v2-preview-title" className="v2-preview">
      <div className="v2-dialog-body v2-preview-body">
        <div className="v2-preview-head">
          <span className="v2-row-text">
            <h2 id="v2-preview-title" className="v2-dialog-title v2-ellipsis">{image.name}</h2>
            <span className="v2-card-note v2-ellipsis">
              {image.detail}
              {image.note ? ` · ${image.note}` : ""}
            </span>
          </span>
          <Button variant="secondary" size="sm" onClick={onClose}>Close</Button>
        </div>
        <Large key={image.id} source={source} image={image} />
        {images.length > 1 && (
          <ul className="v2-preview-strip" aria-label="In this group">
            {images.map((other, index) => (
              <li key={other.id} className={index === at ? "on" : ""}>
                <button type="button" aria-label={`Show ${other.name}`} aria-current={index === at} onClick={() => setAt(index)}>
                  <Picture source={source} id={other.id} size={56} label={other.name} fallback={<IconTile tone="violet" icon="file" size={56} />} />
                </button>
                {other.note && <span className="v2-preview-note">{other.note}</span>}
              </li>
            ))}
          </ul>
        )}
      </div>
    </Modal>
  );
}
