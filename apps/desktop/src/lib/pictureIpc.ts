import { ContractError } from "../contracts/systemMetrics";
import { call } from "./storageIpc";

/** Which page's row a picture is for (src-tauri `PictureSource`). For `browser`, the ID is the
 * browser's name (`safari`, `chrome`, `brave`, `edge`). */
export type PictureSource = "files" | "explorer" | "trash" | "app" | "planned" | "browser";

function parsePicture(payload: unknown): string | null {
  if (payload === null) {
    return null;
  }
  if (typeof payload === "string" && /^data:image\/(jpeg|png);base64,[A-Za-z0-9+/=]+$/.test(payload)) {
    return payload;
  }
  throw new ContractError("$");
}

/** The thumbnail (or, for an app, the icon) of row `id`, at most `size` pixels, as a data URL;
 * `null` when there is none. */
export function getPicture(source: PictureSource, id: string, size: number): Promise<string | null> {
  return call("get_picture", { source, id, size: Math.round(size) }, parsePicture);
}
