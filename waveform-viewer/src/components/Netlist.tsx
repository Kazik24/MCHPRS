import { MaterialIcon } from "./MaterialIcon";
import type { Signal, Trace } from "../trace";
import type { Row } from "../state";

interface Props {
  trace: Trace;
  rows: Row[];
  query: string;
  width: number;
  toggleSignal: (signal: Signal) => void;
  closed: string[];
  setClosed: (scopes: string[]) => void;
}

export function Netlist({
  trace,
  rows,
  query,
  width,
  toggleSignal,
  closed,
  setClosed,
}: Props) {
  const fold = (id: string) =>
    setClosed(
      closed.includes(id)
        ? closed.filter((key) => key !== id)
        : [...closed, id],
    );
  return (
    <aside className="sidebar" style={{ width }}>
      <div className="section-title">
        NETLIST <small>{trace.signals.length} probes</small>
      </div>
      <div className="tree-list" role="tree" aria-label="Available probes">
        <div className="tree-node root">
          <MaterialIcon name="expand" size={16} />
          <span className="chip purple">
            <MaterialIcon name="memory" size={16} />
          </span>
          plot_{trace.plot.join("_")}
        </div>
        {[...new Set(trace.signals.map((signal) => signal.scope))].map(
          (scope) => (
            <div key={scope}>
              <button
                className="tree-node scope"
                role="treeitem"
                aria-expanded={!closed.includes(scope)}
                onClick={() => fold(scope)}
              >
                <MaterialIcon
                  name={closed.includes(scope) ? "chevron" : "expand"}
                  size={16}
                />
                <span className="chip purple">
                  <MaterialIcon name="scope" size={16} />
                </span>
                {scope}
              </button>
              {!closed.includes(scope) &&
                trace.signals
                  .filter(
                    (signal) =>
                      signal.scope === scope &&
                      `${scope}.${signal.name}`
                        .toLowerCase()
                        .includes(query.toLowerCase()),
                  )
                  .map((signal) => {
                    const row = rows.find(
                      (item) => item.signalId === signal.id,
                    );
                    return (
                      <div
                        className="tree-node signal"
                        role="treeitem"
                        aria-selected={!!row?.enabled}
                        key={signal.id}
                        title={`${signal.scope}.${signal.name}\nBlock ${signal.position.join(", ")}`}
                      >
                        <input
                          type="checkbox"
                          aria-label={`Display ${signal.name}`}
                          checked={!!row?.enabled}
                          onChange={() => toggleSignal(signal)}
                        />
                        <span className="chip">
                          <MaterialIcon name="signal" size={16} />
                        </span>
                        <span>{signal.name}</span>
                      </div>
                    );
                  })}
            </div>
          ),
        )}
      </div>
    </aside>
  );
}
