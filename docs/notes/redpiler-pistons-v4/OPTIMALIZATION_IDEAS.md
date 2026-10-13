

# Mandatory optimalizations

- Unessesery Dust removal
- Unessesery Updates removal
- Instant detection (Prove piston is instant -> Remove reset circuit, prove equivalence or reject if cannot be proven)
- Instant circuit minimalization - replace repeted buffers with big buffer, parse OR, AND and Negation instant nodes
- Detect BUD switches
- Maybe we can make optional assumption: if we guarantee that after falling edge, the inputs of the instant circuit wont change until reset, then we can make extra deep optimalizations
