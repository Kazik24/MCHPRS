// Independent protocol-770 RedstoneTools command regression. See tools/README.md.
const mc = require("minecraft-protocol");
const data = require("minecraft-data")("1.21.5");
const Chunk = require("prismarine-chunk")("1.21.5");
const Block = require("prismarine-block")("1.21.5");
const { Vec3 } = require("vec3");
const nbt = require("prismarine-nbt");
const assert = require("assert/strict");
const port = Number(process.argv[2] || 25589);
const restart = process.argv.includes("--restart");
const clients = [];
let stopping = false;
let failure;
let sequence = 500;
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(predicate, description) {
  for (let i = 0; i < 100; i++) {
    if (failure) {
      throw failure;
    }
    if (predicate()) {
      return;
    }
    await delay(30);
  }
  throw Error("Timeout: " + description);
}
async function connect(name) {
  const c = mc.createClient({
    host: "127.0.0.1",
    port,
    username: name,
    version: "1.21.5",
    auth: "offline",
  });
  clients.push(c);
  c.chunks = new Map();
  c.blocks = new Map();
  c.inventory = new Map();
  c.menus = new Map();
  c.messages = [];
  c.components = [];
  c.completions = new Map();
  c.objectives = new Set();
  c.scores = new Map();
  c.sidebar = null;
  c.commandTree = null;
  c.acks = [];
  c.currentMenu = null;
  c.contentCount = 0;
  c.slotCount = 0;
  c.on("packet", (p, m) => {
    try {
      if (m.name === "position") {
        c.write("teleport_confirm", { teleportId: p.teleportId });
      }
      if (m.name === "system_chat") {
        const component = nbt.simplify(p.content);
        c.components.push(component);
        c.messages.push(JSON.stringify(component));
      }
      if (m.name === "declare_commands") {
        c.commandTree = p;
      }
      if (m.name === "tab_complete") {
        c.completions.set(p.transactionId, p);
      }
      if (m.name === "scoreboard_display_objective") {
        c.sidebar = p.name;
      }
      if (m.name === "scoreboard_objective") {
        if (p.action === 0) {
          assert(!c.objectives.has(p.name), "duplicate objective " + p.name);
          c.objectives.add(p.name);
        } else if (p.action === 1) {
          assert(
            c.objectives.delete(p.name),
            "missing objective removal " + p.name,
          );
          c.scores.delete(p.name);
        }
      }
      if (m.name === "scoreboard_score") {
        assert(
          c.objectives.has(p.scoreName),
          "score before objective creation",
        );
        if (!c.scores.has(p.scoreName)) {
          c.scores.set(p.scoreName, new Set());
        }
        c.scores.get(p.scoreName).add(p.itemName);
      }
      if (m.name === "reset_score") {
        c.scores.get(p.objective_name)?.delete(p.entity_name);
      }
      if (m.name === "map_chunk") {
        const chunk = new Chunk({ minY: 0, worldHeight: 256 });
        chunk.load(p.chunkData);
        for (const key of c.blocks.keys()) {
          const [x, , z] = key.split(",").map(Number);
          if (Math.floor(x / 16) === p.x && Math.floor(z / 16) === p.z) {
            c.blocks.delete(key);
          }
        }
        c.chunks.set(`${p.x},${p.z}`, chunk);
      }
      if (m.name === "block_change") {
        c.blocks.set(`${p.location.x},${p.location.y},${p.location.z}`, p.type);
      }
      if (m.name === "multi_block_change") {
        for (const record of p.records) {
          c.blocks.set(
            `${p.chunkCoordinates.x * 16 + ((record >>> 8) & 15)},${p.chunkCoordinates.y * 16 + (record & 15)},${p.chunkCoordinates.z * 16 + ((record >>> 4) & 15)}`,
            record >>> 12,
          );
        }
      }
      if (m.name === "open_window") {
        c.currentMenu = p.windowId;
        c.opened = p;
      }
      if (m.name === "close_window" && c.currentMenu === p.windowId) {
        c.currentMenu = null;
      }
      if (m.name === "window_items") {
        c.contentCount++;
        if (p.windowId === 0) {
          p.items.forEach((item, slot) => c.inventory.set(slot, item));
        } else {
          c.menus.set(p.windowId, p);
        }
      }
      if (m.name === "set_slot") {
        c.slotCount++;
        if (p.windowId === 0) {
          c.inventory.set(p.slot, p.item);
        }
      }
      if (m.name === "acknowledge_player_digging") {
        c.acks.push(p.sequenceId);
      }
    } catch (error) {
      failure = error;
    }
  });
  c.on("error", (error) => {
    if (!stopping) {
      failure = error;
    }
  });
  c.on("kick_disconnect", (p) => {
    if (!stopping) {
      failure = Error("Unexpected disconnect: " + JSON.stringify(p));
    }
  });
  await until(() => c.chunks.has("8,8"), "join " + name);
  return c;
}
function state(c, x, y, z = 130) {
  return (
    c.blocks.get(`${x},${y},${z}`) ??
    c.chunks
      .get(`${Math.floor(x / 16)},${Math.floor(z / 16)}`)
      ?.getBlockStateId(new Vec3(x & 15, y, z & 15))
  );
}
const props = (c, x, y, z) =>
  Block.fromStateId(state(c, x, y, z), 0).getProperties();
const count = (slot) => slot?.itemCount || 0;
const sum = (slots) => slots.reduce((total, slot) => total + count(slot), 0);
const menu = (c) => c.menus.get(c.currentMenu);
function move(c, x, y, z = 130) {
  c.write("position", {
    x,
    y,
    z,
    flags: { onGround: false, hasHorizontalCollision: false },
  });
}
async function cmd(c, command, text) {
  const start = c.messages.length;
  c.write("chat_command", { command });
  await until(
    () => c.messages.slice(start).some((m) => m.includes(text)),
    command + " response",
  );
}
function item(name, itemCount = 1, components = []) {
  return {
    itemCount,
    itemId: data.itemsByName[name].id,
    addedComponentCount: components.length,
    removedComponentCount: 0,
    components,
    removeComponents: [],
  };
}
async function creative(c, slot, item) {
  const previous = c.slotCount;
  c.write("set_creative_slot", { slot, item });
  await until(() => c.slotCount > previous, "creative inventory echo");
}
async function use(c, x, y, z = 130, { face = 1, cursorY = 1 } = {}) {
  const seq = sequence++;
  c.write("block_place", {
    hand: 0,
    location: { x, y, z },
    direction: face,
    cursorX: 0.5,
    cursorY,
    cursorZ: 0.5,
    insideBlock: false,
    worldBorderHit: false,
    sequence: seq,
  });
  await until(() => c.acks.includes(seq), "use acknowledgement");
}

async function place(c, x, y, z = 130) {
  await use(c, x, y - 1, z);
}
async function openChest(c, x, y, z = 130) {
  await use(c, x, y, z);
  await until(
    () => c.currentMenu !== null && c.menus.has(c.currentMenu),
    "container opens",
  );
  assert.equal(c.opened.inventoryType, 2, "correct chest menu");
  assert.equal(menu(c).items.length, 27 + 36, "chest + 36 player slots");
}
async function close(c) {
  const id = c.currentMenu;
  c.write("close_window", { windowId: id });
  await until(() => c.currentMenu === null, "menu close");
}
async function setBlock(c, x, y, name, z = 130) {
  move(c, x, y, z);
  await cmd(c, "/pos1", "First paw");
  await cmd(c, "/pos2", "Second paw");
  await cmd(c, "/set " + name, "Operation complete");
}

async function select(c, first, second = first) {
  move(c, ...first);
  await cmd(c, "/pos1", "First paw");
  move(c, ...second);
  await cmd(c, "/pos2", "Second paw");
}

function actions(component) {
  const found = [];
  if (component.click_event) {
    found.push(component.click_event.command);
  }
  for (const child of component.extra || []) {
    found.push(...actions(child));
  }
  return found;
}

async function complete(c, text) {
  const transactionId = sequence++;
  c.write("tab_complete", { transactionId, text });
  await until(() => c.completions.has(transactionId), "completion " + text);
  return c.completions.get(transactionId);
}

function component(slot, name) {
  return slot?.components?.find((value) => value.type === name);
}

(async () => {
  const a = await connect("ToolsSmokeOne");
  let floor = 0;
  for (let y = 0; y < 256; y++) {
    if (data.blocksByStateId[state(a, 128, y, 128)]?.name !== "air") {
      floor = y;
    }
  }
  const y = floor + 1;
  a.write("held_item_slot", { slotId: 0 });
  move(a, 128, y, 128);
  await until(() => a.commandTree !== null, "command declarations");
  const root = a.commandTree.nodes[a.commandTree.rootIndex];
  const names = root.children.map(
    (index) => a.commandTree.nodes[index].extraNodeData?.name,
  );
  for (const name of [
    "/find",
    "/signsearch",
    "/ss",
    "/rstack",
    "/rs",
    "container",
    "cursel",
  ]) {
    assert(names.includes(name), "declared command " + name);
  }
  assert(
    !names.includes("autowire") && !names.includes("aw"),
    "autowire removed",
  );

  assert(!names.includes("slab"), "redundant slab command removed");

  if (restart) {
    assert.equal(
      a.sidebar,
      "redpiler_status",
      "selection mode is session-only",
    );
    assert(!a.objectives.has("rf_selection"));
    assert.equal(
      a.inventory.get(36).itemCount,
      32,
      "ordinary slab count survives restart",
    );
    move(a, 142, y, 136);
    await place(a, 144, y, 136);
    await until(
      () => props(a, 144, y, 136).type === "top",
      "ordinary slab still places on top after restart",
    );
    assert(
      component(a.inventory.get(36), "unbreakable"),
      "unrelated slab component survives restart",
    );
    assert(
      component(a.inventory.get(37), "custom_name"),
      "container name survives restart",
    );
    assert(
      component(a.inventory.get(37), "lore"),
      "container lore survives restart",
    );
    await cmd(a, "/find -p 1", "No cached search results yet");
    await creative(a, 36, { itemCount: 0 });
    move(a, 148, y, 130);
    await openChest(a, 150, y);
    assert.equal(
      sum(menu(a).items.slice(0, 27)),
      1728,
      "chest contents survive restart",
    );
    await close(a);
  } else {
    const b = await connect("ToolsSmokeTwo");
    move(b, 128, y, 128);
    await cmd(a, "rtps 0", "circuit's new tick pace is set");
    await cmd(a, "help tools", "//find <block>");
    await cmd(a, "/help rs", "//rs [direction]");

    await select(a, [130, y, 130], [138, y, 130]);
    await cmd(a, "/set repeater[facing=north]", "Operation complete");
    await setBlock(a, 134, y, "repeater[facing=east]");
    await select(a, [130, y, 130], [138, y, 130]);
    const foundStart = a.components.length;
    await cmd(a, "/find repeater", "9 matches");
    await until(
      () =>
        a.components.slice(foundStart).flatMap(actions).includes("//find -p 2"),
      "clickable next page",
    );
    assert(
      a.components
        .slice(foundStart)
        .flatMap(actions)
        .includes(`/tp 130 ${y + 1} 130`),
    );
    const secondPage = a.components.length;
    await cmd(a, "/find -p 2", "Page 2/2");
    await until(
      () =>
        a.components
          .slice(secondPage)
          .flatMap(actions)
          .includes(`/tp 138 ${y + 1} 130`),
      "stable second-page positions",
    );
    await cmd(a, "/find missing_block", "Couldn't sniff out a block");
    await cmd(a, "/find -p 2", "Page 2/2");
    await cmd(a, "/find -p 0", "Page numbers start at 1");
    await cmd(a, "/find -p 99", "result trail has pages 1 through");
    assert.deepEqual(
      (await complete(a, "//find -p ")).matches.map((match) => match.match),
      ["1", "2"],
    );
    assert(
      (await complete(a, "/container ch")).matches.some(
        (match) => match.match === "chest",
      ),
    );
    assert(
      (await complete(a, "/container chest ")).matches.some(
        (match) => match.match === "f",
      ),
    );
    await cmd(a, "/find repeater[facing=north]", "8 matches");
    await cmd(a, "/find diamond_block", "no matches");
    await cmd(a, "/find -p 1", "no matches");
    await cmd(a, "/find repeater", "9 matches");

    await setBlock(a, 140, y, "oak_sign");
    const location = { x: 140, y, z: 130 };
    a.write("update_sign", {
      location,
      isFrontText: true,
      text1: "énya front",
      text2: "",
      text3: "",
      text4: "",
    });
    a.write("update_sign", {
      location,
      isFrontText: false,
      text1: "back nya",
      text2: "",
      text3: "",
      text4: "",
    });
    await select(a, [140, y, 130]);
    const signs = a.components.length;
    await cmd(a, "/ss nya", "1 matches");
    await until(
      () =>
        a.messages.slice(signs).some((message) => message.includes("back 1")),
      "both sign sides",
    );
    const rows = a.components
      .slice(signs)
      .flatMap((value) => value.extra || []);
    assert(
      rows.some((value) => value.text === "nya" && value.color === "yellow"),
      "actual regex range highlighted",
    );
    assert(
      rows.some((value) => value.text === "é"),
      "unicode prefix preserved",
    );
    await cmd(a, "/ss [", "Invalid regular expression");
    await cmd(a, "/ss -p 1", "1 matches");

    await creative(a, 36, item("diamond", 5));
    const occupied = JSON.stringify(a.inventory.get(36));
    await cmd(a, "container b 0", "your item's fetched");
    assert.equal(
      JSON.stringify(a.inventory.get(36)),
      occupied,
      "occupied held slot is preserved",
    );
    assert.equal(a.inventory.get(37).itemId, data.itemsByName.barrel.id);
    assert(component(a.inventory.get(37), "custom_name"));
    const inventoryBefore = JSON.stringify([...a.inventory]);
    await cmd(a, "container unknown 1", "Can't fetch that container type");
    await cmd(a, "container chest 16", "0..15 or lowercase a..f");
    assert.equal(
      JSON.stringify([...a.inventory]),
      inventoryBefore,
      "invalid item commands leave inventory unchanged",
    );

    await creative(
      a,
      36,
      item("oak_slab", 32, [{ type: "unbreakable", data: Buffer.alloc(0) }]),
    );
    assert.equal(a.inventory.get(36).itemCount, 32);
    assert(component(a.inventory.get(36), "unbreakable"));
    move(a, 142, y, 130);
    await place(a, 144, y, 130);
    await until(() => props(a, 144, y).type === "top", "top slab placed");
    await setBlock(a, 144, y - 1, "air");
    move(a, 142, y, 130);
    await use(a, 144, y);
    await until(
      () => props(a, 144, y + 1).type === "top",
      "top click places above existing slab",
    );
    assert.equal(state(a, 144, y - 1), 0, "top click leaves lower cell empty");
    await use(a, 144, y, 130, { face: 0, cursorY: 0.5 });
    await until(
      () => props(a, 144, y - 1).type === "top",
      "bottom click places top slab beneath existing slab",
    );
    await use(a, 144, y, 130, { face: 5, cursorY: 0.25 });
    await until(
      () => props(a, 145, y).type === "top",
      "lower side click still places on top",
    );

    for (const [index, name] of [
      "smooth_stone_slab",
      "quartz_slab",
      "pale_oak_slab",
      "cut_copper_slab",
      "tuff_slab",
    ].entries()) {
      await creative(a, 36, item(name));
      move(a, 142, y, 134);
      await place(a, 144 + index, y, 134);
      await until(
        () => props(a, 144 + index, y, 134).type === "top",
        name + " places on top without components",
      );
    }

    await creative(a, 36, { itemCount: 0 });
    await cmd(a, "container c f", "your item's fetched");
    const savedChest = a.inventory.get(36);
    assert(component(savedChest, "container"));
    assert(component(savedChest, "enchantment_glint_override"));
    move(a, 148, y, 130);
    await place(a, 150, y, 130);
    await until(
      () => data.blocksByStateId[state(a, 150, y)]?.name === "chest",
      "chest placed",
    );
    await creative(a, 36, { itemCount: 0 });
    await openChest(a, 150, y);
    assert.equal(
      sum(menu(a).items.slice(0, 27)),
      1728,
      "chest has requested comparator contents",
    );
    await close(a);

    for (let slot = 9; slot < 45; slot++) {
      await creative(a, slot, item("stone"));
    }
    const fullInventory = JSON.stringify([...a.inventory]);
    await cmd(a, "container chest 1", "inventory is full");
    assert.equal(
      JSON.stringify([...a.inventory]),
      fullInventory,
      "full inventory commands preserve every slot",
    );
    // Restore generated items with server commands after freeing their slots.
    await creative(
      a,
      36,
      item("oak_slab", 32, [{ type: "unbreakable", data: Buffer.alloc(0) }]),
    );
    await creative(a, 37, { itemCount: 0 });
    await cmd(a, "container chest f", "your item's fetched");

    await select(a, [130, y, 130], [138, y, 130]);
    await cmd(a, "cursel", "pawprints are on the sidebar");
    await until(() => a.sidebar === "rf_selection", "selection sidebar");
    assert(
      !b.objectives.has("rf_selection"),
      "other player keeps Redpiler sidebar",
    );
    assert(
      [...a.scores.get("rf_selection")].some((text) =>
        text.includes("Volume: 9"),
      ),
    );
    await select(b, [120, y, 130], [121, y, 130]);
    await cmd(b, "cursel", "pawprints are on the sidebar");
    await until(
      () =>
        [...(b.scores.get("rf_selection") || [])].some((text) =>
          text.includes("Volume: 2"),
        ),
      "second player selection",
    );
    await cmd(a, "cursel", "sidebar out of sight");
    await until(() => a.sidebar === "redpiler_status", "Redpiler restored");
    assert.equal(b.sidebar, "rf_selection");
    await cmd(a, "/find -p 2", "Page 2/2");
    await cmd(b, "cursel", "sidebar out of sight");
    await cmd(a, "cursel", "pawprints are on the sidebar");

    await select(a, [254, y, 130]);
    const beforeBounds = state(a, 254, y, 130);
    await cmd(a, "/rs east 2 1", "inside this plot");
    assert.equal(
      state(a, 254, y, 130),
      beforeBounds,
      "invalid stack preserves source",
    );
    await setBlock(a, 160, y, "stone");
    await select(a, [160, y, 130]);
    await cmd(a, "/rs -e neu 2 1", "Piled up 2 copies");
    await until(
      () => data.blocksByStateId[state(a, 162, y + 2, 128)]?.name === "stone",
      "diagonal stack",
    );
    await until(
      () =>
        [...(a.scores.get("rf_selection") || [])].some((text) =>
          text.includes("Volume: 27"),
        ),
      "expanded selection volume",
    );
    a.write("chat_command", { command: "/undo" });
    await until(() => state(a, 162, y + 2, 128) === 0, "stack undo");
    a.write("chat_command", { command: "/redo" });
    await until(
      () => data.blocksByStateId[state(a, 162, y + 2, 128)]?.name === "stone",
      "stack redo",
    );
  }
  stopping = true;
  a.write("chat_command", { command: "stop" });
  await delay(400);
  for (const client of clients) {
    client.end();
  }
  console.log(
    "PASS: RedstoneTools commands, caches, components, inventory, placement, chest, independent sidebars" +
      (restart ? " and restart." : "."),
  );
})().catch((error) => {
  console.error(error);
  stopping = true;
  for (const client of clients) {
    client.end();
  }
  process.exit(1);
});
