import { MaterialIcon } from "./MaterialIcon";
import type { MaterialIconName } from "./MaterialIcon";
import type { ScrollMode } from "../scroll";
interface Props {
  action: (name: string) => void;
  search: string;
  setSearch: (value: string) => void;
  snap: boolean;
  setSnap: (value: boolean) => void;
  scrollMode: ScrollMode;
  setScrollMode: (value: ScrollMode) => void;
  busSelected: boolean;
}

export function Toolbar({
  action,
  search,
  setSearch,
  snap,
  setSnap,
  scrollMode,
  setScrollMode,
  busSelected,
}: Props) {
  const button = (name: MaterialIconName, title: string) => (
    <button
      title={title}
      aria-label={title}
      disabled={busSelected && (name === "rising" || name === "falling")}
      onClick={() => action(name)}
    >
      <MaterialIcon name={name} />
    </button>
  );
  return (
    <div className="toolbar" role="toolbar" aria-label="Waveform navigation">
      {button("fit", "Zoom to fit (Ctrl+0)")}
      {button("out", "Zoom out")}
      {button("in", "Zoom in")}
      <span className="tool-divider" />
      {button("first", "Align view to capture start (Home)")}
      {button("previous", "Previous edge")}
      {button("rising", "Next rising edge")}
      {button("falling", "Next falling edge")}
      {button("next", "Next edge")}
      {button("last", "Align view to capture end (End)")}
      <span className="tool-divider" />
      <div className="search">
        <input
          aria-label="Find signal or go to tick"
          placeholder="Search / go to tick"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") action("search");
          }}
        />
      </div>
      {button("back", "Previous tick (Left)")}
      {button("forward", "Next tick (Right)")}
      <button
        className={snap ? "active" : ""}
        aria-label="Snap cursor to nearest edge"
        aria-pressed={snap}
        title="Snap cursor to nearest edge"
        onClick={() => setSnap(!snap)}
      >
        <MaterialIcon name="snap" />
      </button>
      <select
        aria-label="Scrolling mode"
        title="Mouse wheel / trackpad behavior"
        value={scrollMode}
        onChange={(event) => setScrollMode(event.target.value as ScrollMode)}
      >
        <option value="auto">auto</option>
        <option value="mouse">mouse</option>
        <option value="touchpad">trackpad</option>
      </select>
      <span className="toolbar-spacer" />
      {button("help", "Mouse and keyboard controls")}
      {button("menu", "Viewer menu")}
    </div>
  );
}
