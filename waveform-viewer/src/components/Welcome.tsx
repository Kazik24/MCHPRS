import type { EmptyState } from "../empty";

export function Welcome({ state }: { state: EmptyState }) {
  return (
    <section className="empty-welcome" aria-label="Plot status">
      <div className="welcome-card">
        <p>{state.message}</p>
        {state.commands && (
          <pre>
            <code>{state.commands.join("\n")}</code>
          </pre>
        )}
      </div>
    </section>
  );
}
