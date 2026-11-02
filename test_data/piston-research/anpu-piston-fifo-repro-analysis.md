# ANPU piston/repeater crop analysis

## Identification

The crop comes from the `anpu_pong` fixture, whose source schematic is `Q2CK@Q2CK_Anpu1_Pong_KBTV.schem`. It is an internal redstone logic and timing section of that Pong computer. The crop identifies the physical topology, but does not establish a named logical unit such as a register or controller.

The [saved schematic](anpu-piston-fifo-repro.schem) is the interpreter's block state immediately before the tick-218 scheduler divergence. Its world-space bounds are **x=88..112, y=49..58, z=118..129**, giving dimensions **25 × 10 × 12**. It contains 687 non-air blocks, mostly black concrete support, redstone wire, repeaters, and red concrete/glass.

## Topology and geometry

- **Four east–west signal lanes** occupy z=118, 120, 122, and 124 on the y=55 layer. They repeat at a two-block Z pitch across the center of the crop. The lane details vary, with dust and repeater routing at different heights.
- At the two ends of these lanes, repeaters around x=89 and x=111 face inward and use a two-tick delay. The z=124 lane also has inward-facing one-tick repeaters at x=92 and x=108.
- **Three sticky pistons** form a bank below the lanes at (101,50,119), (101,50,121), and (101,50,123). All are extended and face west; their heads are at x=100. Their Z positions alternate between the four signal lanes.
- **A torch layer** sits around y=53. Lit floor torches at x=100 align with the four even-Z lanes (118, 120, 122, 124); unlit wall torches at x=100 occupy the intervening odd-Z positions (119, 121, 123).
- Additional dust, repeaters, torches, and supports occupy y=49..58. The signal paths therefore use several vertical levels, with wire connections that rise between layers.

In plan view, the repeated arrangement is a four-lane comb with the piston bank interleaved below it:

```text
                         X (west ← / east →)
Z=118   ─────────────── signal lane ───────────────
Z=119            [sticky piston facing west]
Z=120   ─────────────── signal lane ───────────────
Z=121            [sticky piston facing west]
Z=122   ─────────────── signal lane ───────────────
Z=123            [sticky piston facing west]
Z=124   ─────────────── signal lane ───────────────
```

The diagram shows the repeated footprint; the actual schematic includes cross-connections and routing above and below these rows.

## Regression location

The tick-218 pending-tick order mismatch first differs at the repeaters on the z=124 lane: the compiled queue starts with **(108,55,124)**, while the interpreter queue starts with **(92,55,124)**. These repeaters face toward each other across the lane. This places the observed mismatch on the outer signal structure adjacent to the piston bank, rather than at one of the three piston blocks themselves.

The regression is [`anpu_piston_repeater_fifo_order_in_schematic_crop`](../../crates/core/src/redpiler/tests/sampled/cpu_trace.rs#L94). Run it with:

```powershell
cargo test -p mchprs_core --lib anpu_piston_repeater_fifo_order_in_schematic_crop -- --ignored --nocapture
```

The `.schem` stores only the tick-217 block snapshot; it does not encode the preceding tick history or pending-tick queue. The regression replays the original ANPU fixture to reach that state, checks the crop against it, then compares scheduled ticks within the crop.

## Interpretation limits

The crop supports identifying this as a multi-level repeater/torch logic section with a three-piston actuator bank. It does not, by itself, reveal the section's exact role in the Pong computer; tracing surrounding circuitry would be needed to name that function.
