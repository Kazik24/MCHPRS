"""Run with: py tools/test_physical_piston_scope.py"""
from inspect_physical_piston_scope import candidate_scope

cells = {
    (0, 0, 0): "minecraft:piston[extended=false,facing=east]",
    **{(x, 0, 0): "minecraft:redstone_wire[power=0]" for x in (2, 4, 6, 9)},
}
before = cells.copy()
scope = candidate_scope(cells)
assert scope["candidate_motion_cells"] == 2  # Empty ordinary actor: base and Near.
assert scope["motion_sensitive_dust_seeds"] == 1
assert scope["dust_notification_closure"] == 3  # Reach through possible callbacks.
assert scope["dust_outside_closure"] == 1
assert cells == before
translated = {tuple(a + b for a, b in zip(pos, (8, 8, 8))): state for pos, state in cells.items()}
assert candidate_scope(translated) == scope
cells[(0, 0, 0)] = "minecraft:sticky_piston[extended=false,facing=east]"
assert candidate_scope(cells)["candidate_motion_cells"] == 3
print("physical scope checks passed")
