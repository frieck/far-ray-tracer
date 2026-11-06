import { formatNumber, formatTime } from "../format";
import type { CausticReport } from "../types";

/** Below this, the estimate is noisy and shows up as blotches on surfaces. */
const LOW_PHOTONS_PER_LOOKUP = 100;

function count(n: number): string {
  return n >= 1e6 ? `${formatNumber(n / 1e6)} million` : formatNumber(n);
}

interface RowProps {
  label: string;
  value: string;
  hint: string;
}

function Row({ label, value, hint }: RowProps) {
  return (
    <div title={hint}>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

export function CausticStats({ report }: { report: CausticReport }) {
  const share = report.emitted ? Math.round((report.stored / report.emitted) * 100) : 0;
  const perLookup = Math.round(report.photons_per_lookup);
  return (
    <div className="stats">
      <h2>Photon map</h2>
      <dl>
        <Row
          label="Emitted"
          value={count(report.emitted)}
          hint="Photons sent from the lights toward glass and mirrors."
        />
        <Row
          label="Stored"
          value={`${count(report.stored)} (${share}%)`}
          hint="Photons that reached a lit surface after passing through glass or bouncing off a mirror."
        />
        <Row label="Tracing" value={formatTime(report.trace_ms)} hint="Time spent following the photons." />
        <Row
          label="Memory"
          value={`${formatNumber(report.memory_bytes / 1e6)} MB`}
          hint="Size of the photon map."
        />
        <Row
          label="Lookups"
          value={count(report.lookups)}
          hint="Times the render read the photon map to light a surface."
        />
        <Row
          label="Photons per lookup"
          value={formatNumber(perLookup)}
          hint="Average photons found around each point. More is smoother."
        />
      </dl>
      {perLookup < LOW_PHOTONS_PER_LOOKUP && (
        <p className="note">Few photons per lookup: surfaces may look blotchy. Add photons or raise the radius.</p>
      )}
    </div>
  );
}
