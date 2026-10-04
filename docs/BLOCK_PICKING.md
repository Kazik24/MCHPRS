# Middle-click block picking

Minecraft 1.21.5 sends `pick_item_from_block` (0x22: packed position, include-data boolean) and `pick_item_from_entity` (0x23: entity ID, include-data boolean). Both packets are decoded explicitly. MCHPRS currently has player entities only; these have no pick result.

Creative players can pick blocks within Java's six-block pick tolerance. Out-of-plot, invalid-height, air, moving-placeholder and distant targets are ignored. Existing matching stacks are selected in the hotbar or swapped from the main inventory. Otherwise a one-item stack is inserted into an empty hotbar slot, or the selected slot; displaced items are preserved in an empty main-inventory slot when available. Inventory, selected slot and equipment are synchronized to clients.

Redstone wire picks redstone dust, wall torches/signs pick their item variants, and piston heads pick the corresponding piston. Ctrl-middle-click preserves supported block-entity data, including sign text and container contents. Ordinary picking uses the item's default state rather than copying facing/power properties.

Reference: the checked-in protocol schema and Mojang's exact 1.21.5 server artifact identified in `mc_data/1.21.5/sources.json`, inspected with the official mappings (`ServerGamePacketListenerImpl.handlePickItemFromBlock`, `tryPickItem`, and `Inventory.getSuitableHotbarSlot`). Enchantment-aware replacement priority and pick results for future non-player entities are outside this initial implementation.

Tests cover packet dispatch/truncation, reach validation, stack selection/swapping/preservation, and independent clients picking a piston, reusing a stack, selecting redstone and retaining sign data.
