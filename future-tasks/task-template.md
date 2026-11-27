# Task Template

**Status**: [Planned | In Progress | Blocked | Completed]  
**Priority**: [HIGH | MEDIUM | LOW] ([Type])  
**Effort**: [X-Y hours]  
**Date Identified**: YYYY-MM-DD  
**Related Tasks**: Task XXX, Task YYY

---

## Problem Statement

Clear description of the issue or opportunity:
- What's not working or missing?
- What's the business/technical impact?
- Real examples or data supporting the need

**Example**:
> Service X fails when condition Y occurs, causing Z impact. On Jan 8, 2026, this resulted in 85-minute data gap during market hours.

---

## Current State

### Existing Implementation

Describe how the system currently works:
- Architecture overview
- Key code snippets (with line references)
- Configuration or dependencies
- Observed behavior

```python
# Example code showing current implementation
def current_approach():
    # Problematic pattern
    pass
```

**Limitations**:
- Specific issue #1
- Specific issue #2
- Measurable impact

---

## Proposed Solution

### High-Level Approach

Describe the solution strategy:
- What will change?
- Why this approach vs alternatives?
- Key design decisions

### Technical Design

```python
# Example code showing proposed solution
def improved_approach():
    # Better pattern with explanation
    pass
```

**Trade-offs**:
- **Pros**: Benefits 1, 2, 3
- **Cons**: Drawbacks or risks

---

## Implementation Steps

### Phase 1: [Name] (X hours)
- [ ] Specific task 1
- [ ] Specific task 2
- [ ] Specific task 3

### Phase 2: [Name] (Y hours)
- [ ] Specific task 4
- [ ] Specific task 5

### Phase 3: Testing & Validation (Z hours)
- [ ] Unit tests
- [ ] Integration tests
- [ ] Performance validation

---

## Success Criteria

### Performance Targets
- [ ] Metric 1: Current X → Target Y (Z% improvement)
- [ ] Metric 2: Current A → Target B

### Technical Validation
- [ ] Passes all tests
- [ ] No regressions in related systems
- [ ] Documentation updated

### Code Quality
- [ ] Test coverage ≥ 80%
- [ ] No new linter warnings
- [ ] Peer review approved

---

## Files Modified

**New Files**:
- `path/to/new_file.py` (purpose)
- `tests/test_new_feature.py` (test suite)

**Modified Files**:
- `path/to/existing_file.py`
  - Lines X-Y: Description of changes
  - Method `foo()`: Updated logic
  - Added imports: `module1`, `module2`

**Configuration**:
- `config/settings.yaml`: Added setting `new_param`

---

## Testing Requirements

### Unit Tests
```python
def test_feature_behavior():
    """Test description."""
    # Setup: Arrange test data
    # Action: Execute function
    # Assert: Verify expected outcome
```

### Integration Tests
```python
def test_end_to_end_flow():
    """Test description."""
    # Setup: Initialize system
    # Action: Run full workflow
    # Assert: Validate results
```

### Performance Tests
- Benchmark X should complete in <Y seconds
- Memory usage should not exceed Z MB
- CPU usage increase should be <A%

---

## Expected Impact

### Quantitative Improvements
- **Metric 1**: Current X → Projected Y (Z% improvement)
- **Metric 2**: Reduced A by B%

### Qualitative Benefits
- ✅ Benefit 1
- ✅ Benefit 2
- ✅ Benefit 3

### Risks Mitigated
- Prevents issue X
- Reduces likelihood of Y
- Improves reliability of Z

---

## Dependencies

**Required** (must complete first):
- ✅ Completed Task XXX
- ❌ Blocked by Task YYY

**Recommended** (nice to have):
- Task ZZZ: Would enhance this feature
- Task AAA: Complementary improvement

---

## Risk Mitigation

### Potential Issues

1. **Issue 1**: Description
   - **Mitigation**: Strategy to address

2. **Issue 2**: Description
   - **Mitigation**: Alternative approach

3. **Issue 3**: Description
   - **Mitigation**: Rollback plan

---

## Future Enhancements

Ideas for future iterations (not in scope):
- Enhancement 1: Description
- Enhancement 2: Description
- Related to Task XXX

---

## Notes

Additional context, observations, or lessons learned:
- Note 1
- Note 2
- Related discussions or decisions

---

## References

**Documentation**:
- [FILE.md](../path/to/file.md) - Related documentation
- [ARCHITECTURE.md](../ARCHITECTURE.md) - System design

**External Resources**:
- Link to issue/PR
- Link to research/discussion
- Link to external documentation

---

## Usage Instructions

### Creating a New Task

1. Copy this template to `future-tasks/task-XXX-short-name.md`
2. Fill in all sections (remove placeholders)
3. Update [README.md](README.md) with task entry in Quick Reference table
4. Assign priority (HIGH/MEDIUM/LOW) and effort estimate
5. Link related tasks in both directions

### Working on a Task

1. Update **Status** to "In Progress"
2. Check off implementation steps as completed
3. Run tests and update **Success Criteria** checkboxes
4. Document any deviations in **Notes** section

### Completing a Task

1. Update **Status** to "Completed"
2. Add completion date: `**Date Completed**: YYYY-MM-DD`
3. Move file to `future-tasks/completed/` directory
4. Update [README.md](README.md) to mark task as completed
5. Create entry in `COMPLETED_FEATURES.md` with summary

---

## Checklist

Before considering task complete:
- [ ] All implementation steps checked off
- [ ] All success criteria met
- [ ] Tests passing (unit, integration, performance)
- [ ] Documentation updated (code comments, README, etc.)
- [ ] Peer review completed
- [ ] Deployed to production (if applicable)
- [ ] Monitoring/alerts configured (if applicable)
- [ ] Task marked as completed in tracking system
