#!/usr/bin/env python3
"""
Automatically update all e2e tests to use sandboxed persistent cache.

This script updates test files to:
1. Get sandboxed cache directory using artifacts.get_sandboxed_cache_dir()
2. Add PersistentCachePath parameter to viewer invocations

CRITICAL: This prevents tests from corrupting user's production cache.
"""

import re
from pathlib import Path

# List of test files that need updating
TEST_FILES = [
    'test_cache_simple_poc.py',
    'test_persistent_cache_eviction.py',
    'test_persistentcache_v3_sharded.py',
    'test_hash_collision_handling.py',
    'test_seed_mechanism.py',
    'test_large_rect_cache_strategy.py',
    'test_cpp_cache_back_to_back.py',
    'test_lossy_lossless_parity.py',
    'test_large_rect_lossy_first_hit.py',
    'test_persistent_cache_bandwidth.py',
    'test_cpp_cache_eviction.py',
    'test_cache_parity.py',  # Already has custom path, update to use helper
]

def update_test_file(filepath):
    """Update a single test file to use sandboxed cache."""
    print(f"\nUpdating {filepath.name}...")
    
    with open(filepath, 'r') as f:
        content = f.read()
    
    original_content = content
    changes = []
    
    # Pattern 1: Find viewer invocations with PersistentCache=1 but no path
    # Look for: 'PersistentCache=1' without 'PersistentCachePath'
    
    # Pattern 2: Update existing custom cache paths to use helper
    # Replace: artifacts.logs_dir / 'some_cache' with artifacts.get_sandboxed_cache_dir()
    if 'artifacts.logs_dir' in content and '_cache' in content:
        # This is test_cache_parity.py style
        content = re.sub(
            r"pc_cache_path = artifacts\.logs_dir / '[^']*cache[^']*'",
            "pc_cache_path = artifacts.get_sandboxed_cache_dir()",
            content
        )
        if content != original_content:
            changes.append("Updated to use artifacts.get_sandboxed_cache_dir() helper")
            original_content = content
    
    # Pattern 3: Add cache_dir setup before viewer starts
    # Look for viewer startup with PersistentCache=1
    lines = content.split('\n')
    new_lines = []
    i = 0
    
    while i < len(lines):
        line = lines[i]
        new_lines.append(line)
        
        # Check if this line starts a viewer with PersistentCache
        if ("subprocess.Popen" in line or "viewer_proc = subprocess.Popen" in line) and i + 5 < len(lines):
            # Look ahead for PersistentCache=1
            next_few = '\n'.join(lines[i:min(i+10, len(lines))])
            if "'PersistentCache=1'" in next_few or '"PersistentCache=1"' in next_few:
                if "'PersistentCachePath=" not in next_few and '"PersistentCachePath=' not in next_few:
                    # Need to add cache path!
                    # Find the line with the command array
                    j = i
                    while j < min(i + 10, len(lines)):
                        if 'PersistentCache=1' in lines[j]:
                            # Insert cache setup before Popen
                            indent = ' ' * (len(lines[i]) - len(lines[i].lstrip()))
                            
                            # Add cache dir setup before this Popen
                            insert_pos = i
                            while insert_pos > 0 and lines[insert_pos - 1].strip().endswith('\\'):
                                insert_pos -= 1
                            if insert_pos > 0:
                                insert_pos -= 1
                            
                            # Go back to find good insertion point
                            while insert_pos > 0 and (lines[insert_pos].strip() == '' or lines[insert_pos].strip().startswith('#')):
                                insert_pos -= 1
                            insert_pos += 1
                            
                            # Add the sandboxing code
                            new_block = [
                                '',
                                indent + '# SANDBOXED: Use test-specific cache (not production cache)',
                                indent + 'cache_dir = artifacts.get_sandboxed_cache_dir()',
                            ]
                            
                            # We need to modify the command to add PersistentCachePath
                            # This is tricky - just flag it for manual review
                            changes.append(f"Line {i+1}: Found PersistentCache=1 without path - NEEDS MANUAL UPDATE")
                            break
                        j += 1
        
        i += 1
    
    if changes:
        print(f"  Changes detected:")
        for change in changes:
            print(f"    - {change}")
        
        # Only write if we made actual substitutions (not just manual flags)
        if content != original_content:
            with open(filepath, 'w') as f:
                f.write(content)
            print(f"  ✓ Updated {filepath.name}")
            return True
        else:
            print(f"  ⚠ Manual update required for {filepath.name}")
            return False
    else:
        print(f"  - No changes needed (already sandboxed or no PersistentCache)")
        return None

def main():
    tests_dir = Path(__file__).parent
    
    print("=" * 70)
    print("E2E Test Persistent Cache Sandboxing")
    print("=" * 70)
    print("\nThis script updates tests to use sandboxed cache directories.")
    print("CRITICAL: Prevents corrupting user's production cache!\n")
    
    updated = []
    manual = []
    skipped = []
    
    for test_file in TEST_FILES:
        filepath = tests_dir / test_file
        if not filepath.exists():
            print(f"\n⚠ {test_file} not found, skipping")
            continue
        
        result = update_test_file(filepath)
        if result is True:
            updated.append(test_file)
        elif result is False:
            manual.append(test_file)
        else:
            skipped.append(test_file)
    
    print("\n" + "=" * 70)
    print("SUMMARY")
    print("=" * 70)
    print(f"✓ Auto-updated: {len(updated)}")
    for f in updated:
        print(f"  - {f}")
    
    print(f"\n⚠ Need manual update: {len(manual)}")
    for f in manual:
        print(f"  - {f}")
    
    print(f"\n- Already OK: {len(skipped)}")
    for f in skipped:
        print(f"  - {f}")
    
    if manual:
        print("\n" + "=" * 70)
        print("MANUAL UPDATE INSTRUCTIONS")
        print("=" * 70)
        print("For each file needing manual update:")
        print("1. Add before viewer Popen: cache_dir = artifacts.get_sandboxed_cache_dir()")
        print("2. Add to viewer params: f'PersistentCachePath={cache_dir}'")
    
    return 0 if not manual else 1

if __name__ == '__main__':
    import sys
    sys.exit(main())
