#!/usr/bin/env python3
"""Benchmark grow-divide at increasing simulation durations."""
import time
import sys
sys.path.insert(0, '/home/youdonotexist/code/process-bigraph')
sys.path.insert(0, '/home/youdonotexist/code/bigraph-schema')

from process_bigraph import allocate_core, Composite
from process_bigraph.processes.growth_division import grow_divide_agent

def run_bench(duration, core):
    initial_mass = 1.0
    grow_divide = grow_divide_agent(
        {'grow': {'rate': 0.03}},
        {},
        ['environment', '0'])

    environment = {
        'environment': {
            '0': {
                'mass': initial_mass,
                'grow_divide': grow_divide}}}

    composite = Composite({
        'state': environment,
        'bridge': {
            'inputs': {'environment': ['environment']}}},
        core=core)

    start = time.time()
    composite.update({'environment': {'0': {'mass': 1.0}}}, duration)
    elapsed_ms = (time.time() - start) * 1000

    n_agents = len(composite.state.get('environment', {}))
    return elapsed_ms, n_agents

def main():
    core = allocate_core()
    print("duration,n_agents,wall_ms")
    for dur in [10, 25, 50, 75, 100, 125, 150, 170]:
        ms, n = run_bench(float(dur), core)
        print(f"{dur},{n},{ms:.0f}")
        print(f"  t={dur}: {n} agents, {ms:.0f}ms", file=sys.stderr)

if __name__ == '__main__':
    main()
