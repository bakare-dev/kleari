import { Glyph } from "../ui/icons";

/**
 * Marker attribute on the development badge. scripts/check-release-bundle.mjs fails the
 * build if this string appears in the release bundle, which proves no development-only code
 * ships.
 */
export const SAMPLE_DATA_MARKER = "kleari-sample-data";

/** Says this is a development build. Rendered only in development builds; every screen,
 * the menu-bar popover included, uses live data. */
export function SampleBadge() {
  return (
    <div className="v2-sample-root" data-sample={SAMPLE_DATA_MARKER}>
      <span className="v2-sample">
        <Glyph name="info" size={14} />
        <span>Development build · live data</span>
      </span>
    </div>
  );
}
