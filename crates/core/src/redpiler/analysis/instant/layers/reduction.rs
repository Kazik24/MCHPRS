//! Keep the backward slice of observable roots; construction pins and unused inputs are cold.
use super::*;

pub(crate) fn reduce(region: &LayeredRegion, monitor: &TaskMonitor) -> Result<ReducedLayerProgram, String> {
    let mut roots = vec![(LayerRoot::StationaryEntry, region.stationary_entry)];
    roots.extend(
        region
            .outputs
            .iter()
            .enumerate()
            .map(|(index, output)| (LayerRoot::Output(index), output.emitted_when)),
    );
    for (index, resource) in region.resource_outputs.iter().enumerate() {
        roots.push((LayerRoot::Transfer(index), resource.transfer_when));
        roots.push((LayerRoot::SourceAbsent(index), resource.source_absent_after));
    }
    // Acceptance remains observable to reset/publication even when no immediate external output changes.
    for (layer, frontier) in region.layers.iter().enumerate() {
        for (occurrence, use_) in frontier.uses.iter().enumerate() {
            roots.push((LayerRoot::Accepted { layer, occurrence }, use_.accepted));
        }
    }
    reduce_roots(&region.inputs, &region.expressions, roots, monitor)
}

pub(super) fn reduce_roots(
    inputs: &[LayerInput],
    expressions: &[LayerExpr],
    roots: Vec<(LayerRoot, LayerExprId)>,
    monitor: &TaskMonitor,
) -> Result<ReducedLayerProgram, String> {
    check_cancelled(monitor)?;
    let mut live = vec![false; expressions.len()];
    for (_, root) in &roots {
        *live.get_mut(root.0 as usize).ok_or("observable layer root is missing")? = true;
    }
    // The arena is topological, so one reverse pass marks the complete reachable slice.
    for (index, expression) in expressions.iter().enumerate().rev() {
        check_cancelled(monitor)?;
        let operands: &[LayerExprId] = match expression {
            LayerExpr::Not(value) => std::slice::from_ref(value),
            LayerExpr::All(values) | LayerExpr::Any(values) => values,
            LayerExpr::Input(input) if input.0 as usize >= inputs.len() => return Err("layer expression input is missing".into()),
            _ => &[],
        };
        for operand in operands {
            if operand.0 as usize >= index {
                return Err("layer expression has a missing or forward operand".into());
            }
            if live[index] {
                live[operand.0 as usize] = true;
            }
        }
    }
    let mut remap = vec![LayerExprId(u32::MAX); expressions.len()];
    let mut input_remap = vec![None; inputs.len()];
    let mut reduced_inputs = Vec::new();
    let mut reduced_expressions = Vec::new();
    for (index, expression) in expressions.iter().enumerate() {
        check_cancelled(monitor)?;
        if !live[index] {
            continue;
        }
        let expression = match expression {
            LayerExpr::Constant(value) => LayerExpr::Constant(*value),
            LayerExpr::Input(input) => {
                let id = match input_remap[input.0 as usize] {
                    Some(id) => id,
                    None => {
                        let id = LayerInputId(u32::try_from(reduced_inputs.len()).map_err(|_| "too many reduced layer inputs")?);
                        reduced_inputs.push(inputs[input.0 as usize]);
                        input_remap[input.0 as usize] = Some(id);
                        id
                    }
                };
                LayerExpr::Input(id)
            }
            LayerExpr::Not(value) => LayerExpr::Not(remap[value.0 as usize]),
            LayerExpr::All(values) => LayerExpr::All(values.iter().map(|value| remap[value.0 as usize]).collect()),
            LayerExpr::Any(values) => LayerExpr::Any(values.iter().map(|value| remap[value.0 as usize]).collect()),
        };
        remap[index] = LayerExprId(u32::try_from(reduced_expressions.len()).map_err(|_| "too many reduced layer operations")?);
        reduced_expressions.push(expression);
    }
    let roots = roots.into_iter().map(|(kind, root)| (kind, remap[root.0 as usize])).collect();
    Ok(ReducedLayerProgram {
        inputs: reduced_inputs.into_boxed_slice(),
        expressions: reduced_expressions.into_boxed_slice(),
        roots,
    })
}

#[cfg(test)]
mod tests;
