import { ReleaseApp } from "./ReleaseApp";
import { SampleBadge } from "./sample/SampleBadge";

/**
 * The v2 interface in development builds only: main.tsx loads this module only when
 * import.meta.env.DEV is true. It is the release interface, marked as a development build.
 */
export function V2App() {
  return <ReleaseApp footer={() => <SampleBadge />} />;
}
