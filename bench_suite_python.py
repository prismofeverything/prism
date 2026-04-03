#!/usr/bin/env python3
"""Benchmark Python spatio-flux test suite — timing only, no plots."""

import sys
import os
sys.path.insert(0, os.path.expanduser('~/code/spatio-flux'))
sys.path.insert(0, os.path.expanduser('~/code/process-bigraph'))
sys.path.insert(0, os.path.expanduser('~/code/bigraph-schema'))

import time
import json
from process_bigraph import allocate_core
from spatio_flux.experiments.test_suite import SIMULATIONS
from spatio_flux.library.tools import run_composite_document

def main():
    core = allocate_core()

    # Match the Rust report's canonical order
    canonical = [
        'monod_kinetics',
        'ecoli_core_dfba',
        'ecoli_dfba',
        'yeast_dfba',
        'community_dfba',
        'dfba_kinetics_community',
        'spatial_many_dfba',
        'spatial_dfba_process',
        'diffusion_process',
        'brownian_particles',
        'br_particles_kinetics',
        'br_particles_dfba',
        'comets_diffusion',
        'comets_br_particles_kinetics',
        'comets_br_particles_dfba',
        'newtonian_particles',
        'comets_nt_particles_dfba',
        'spatioflux_reference_demo',
    ]

    # Map Rust fixture names to Python test names
    name_map = {
        'monod_kinetics': 'kinetics_single',
        'ecoli_core_dfba': 'dfba_single',
        'ecoli_dfba': 'ecoli_dfba',
        'yeast_dfba': 'yeast_dfba',
        'community_dfba': 'community_dfba',
        'dfba_kinetics_community': 'dfba_kinetics_community',
        'spatial_many_dfba': 'spatial_many_dfba',
        'spatial_dfba_process': 'spatial_dfba_process',
        'diffusion_process': 'diffusion_process',
        'brownian_particles': 'brownian_particles',
        'br_particles_kinetics': 'br_particles_kinetics',
        'br_particles_dfba': 'br_particles_dfba',
        'comets_diffusion': 'comets_diffusion',
        'comets_br_particles_kinetics': 'comets_br_particles_kinetics',
        'comets_br_particles_dfba': 'comets_br_particles_dfba',
        'newtonian_particles': 'newtonian_particles',
        'comets_nt_particles_dfba': 'comets_nt_particles_dfba',
        'spatioflux_reference_demo': 'spatioflux_reference_demo',
    }

    results = {}
    total = 0.0

    print(f"{'sim':<35} {'time':>6} {'wall_ms':>10}")
    print("-" * 55)

    for rust_name in canonical:
        py_name = name_map.get(rust_name)
        if py_name not in SIMULATIONS:
            print(f"{rust_name:<35} {'':>6} {'SKIP':>10}")
            continue

        sim_info = SIMULATIONS[py_name]
        config = sim_info.get('config', {})
        runtime = sim_info.get('time', 60)

        try:
            doc = sim_info['doc_func'](core=core, config=config)
            t0 = time.perf_counter()
            run_composite_document(doc, core=core, name=py_name, time=runtime,
                                   show_types=False, show_values=False)
            ms = (time.perf_counter() - t0) * 1000
            results[rust_name] = {'wall_ms': ms, 'sim_time': runtime}
            total += ms
            print(f"{rust_name:<35} {runtime:>5.0f}s {ms:>9.0f}ms")
        except Exception as e:
            print(f"{rust_name:<35} {'':>6} ERROR: {e}")
            results[rust_name] = {'wall_ms': None, 'error': str(e)}

    print("-" * 55)
    print(f"{'TOTAL':<35} {'':>6} {total:>9.0f}ms")

    with open('/tmp/python_suite_bench.json', 'w') as f:
        json.dump(results, f, indent=2)
    print(f"\nSaved to /tmp/python_suite_bench.json")

if __name__ == '__main__':
    main()
