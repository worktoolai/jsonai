#!/usr/bin/env bash
set -euo pipefail

# Integration tests for JSON Pointer escape handling (~1 for / and ~0 for ~)
# This script tests the jsonai CLI with keys containing slashes and tildes.

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

# Track failures
FAILURES=0

# Test helper function
run_test() {
    local test_name="$1"
    local command="$2"
    local expected="$3"

    echo -n "Testing: $test_name... "

    # Capture output and check for expected string
    local output
    output=$(eval "$command" 2>&1) || true

    if echo "$output" | grep -q "$expected"; then
        echo -e "${GREEN}PASS${NC}"
        return 0
    else
        echo -e "${RED}FAIL${NC}"
        echo "  Command: $command"
        echo "  Expected to find: $expected"
        echo "  Actual output:"
        echo "$output" | sed 's/^/    /'
        FAILURES=$((FAILURES + 1))
        return 1
    fi
}

# Create a temporary file for testing
TEMP_FILE=$(mktemp)
trap "rm -f $TEMP_FILE" EXIT

# Initialize test JSON
cat > "$TEMP_FILE" << 'EOF'
{
  "src/lib": {
    "hooks": "old_value",
    "utils": "keep_me"
  },
  "config~backup": {
    "enabled": false
  },
  "path/to/file~name": "original",
  "mixed~1/0": "test"
}
EOF

echo "=========================================="
echo "JSON Pointer Escape Integration Tests"
echo "=========================================="
echo ""

# Build jsonai first
echo "Building jsonai..."
cd /Users/bjm/work/ai/jsonai
cargo build --release --quiet 2>&1 | grep -v "warning:" || true
JSONAI="/Users/bjm/work/ai/jsonai/target/release/jsonai"

if [ ! -f "$JSONAI" ]; then
    echo "Error: Failed to build jsonai"
    exit 1
fi

echo "Using jsonai binary: $JSONAI"
echo ""

# Test 1: Set value with slash in key (src/lib/hooks)
run_test \
    "SET: Update value with slash in key" \
    "$JSONAI set --pointer /src~1lib/hooks '\"new_value\"' $TEMP_FILE --dry-run" \
    '"new_value"'

# Actually update the file for verification
$JSONAI set --pointer /src~1lib/hooks '"new_value"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Verify the file was actually updated
run_test \
    "SET: Verify file contains updated value for slash key" \
    "$JSONAI query --filter '.[\"src/lib\"].hooks' $TEMP_FILE" \
    'new_value'

# Test 2: Set value with tilde in key (config~backup)
run_test \
    "SET: Update value with tilde in key" \
    "$JSONAI set --pointer /config~0backup/enabled 'true' $TEMP_FILE --dry-run" \
    'true'

# Actually update the file
$JSONAI set --pointer /config~0backup/enabled 'true' $TEMP_FILE > /dev/null 2>&1

# Verify
run_test \
    "SET: Verify file contains updated value for tilde key" \
    "$JSONAI query --filter '.[\"config~backup\"].enabled' $TEMP_FILE" \
    'true'

# Test 3: Set value with mixed escapes (path/to/file~name)
run_test \
    "SET: Update value with mixed escapes" \
    "$JSONAI set --pointer /path~1to~1file~0name '\"updated\"' $TEMP_FILE --dry-run" \
    '"updated"'

# Actually update the file
$JSONAI set --pointer /path~1to~1file~0name '"updated"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Verify
run_test \
    "SET: Verify file contains updated value for mixed escapes" \
    "$JSONAI query --filter '.[\"path/to/file~name\"]' $TEMP_FILE" \
    'updated'

# Test 4: Set value with complex mixed escapes (mixed~1/0 is the actual key)
run_test \
    "SET: Update value with complex mixed escapes" \
    "$JSONAI set --pointer /mixed~01~10 '\"complex_updated\"' $TEMP_FILE --dry-run" \
    '"complex_updated"'

# Test 5: Add new key with slash
cat > "$TEMP_FILE" << 'EOF'
{
  "src/lib": {
    "existing": "value"
  }
}
EOF

run_test \
    "ADD: Add new key with slash in parent" \
    "$JSONAI add --pointer /src~1lib/hooks '\"new_hook\"' $TEMP_FILE --dry-run" \
    '"new_hook"'

# Actually add the key
$JSONAI add --pointer /src~1lib/hooks '"new_hook"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Verify the structure
run_test \
    "ADD: Verify added key with slash parent" \
    "$JSONAI query --filter '.[\"src/lib\"].hooks' $TEMP_FILE" \
    'new_hook'

# Test 6: Add new key with tilde
cat > "$TEMP_FILE" << 'EOF'
{
  "existing": "value"
}
EOF

run_test \
    "ADD: Add new key with tilde" \
    "$JSONAI add --pointer /config~0backup 'true' $TEMP_FILE --dry-run" \
    'true'

# Actually add the key
$JSONAI add --pointer /config~0backup 'true' $TEMP_FILE > /dev/null 2>&1

# Verify
run_test \
    "ADD: Verify added key with tilde" \
    "$JSONAI query --filter '.[\"config~backup\"]' $TEMP_FILE" \
    'true'

# Test 7: Add nested key with multiple escapes
cat > "$TEMP_FILE" << 'EOF'
{
  "parent": {}
}
EOF

run_test \
    "ADD: Add nested key with multiple slashes" \
    "$JSONAI add --pointer /parent/path~1to~1deep '\"nested_value\"' $TEMP_FILE --dry-run" \
    '"nested_value"'

# Actually add the key
$JSONAI add --pointer /parent/path~1to~1deep '"nested_value"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Verify
run_test \
    "ADD: Verify added nested key with slashes" \
    "$JSONAI query --filter '.parent[\"path/to/deep\"]' $TEMP_FILE" \
    'nested_value'

# Test 8: Delete key with slash
cat > "$TEMP_FILE" << 'EOF'
{
  "src/lib": {
    "hooks": "delete_me",
    "utils": "keep_me"
  }
}
EOF

run_test \
    "DELETE: Delete key with slash in parent" \
    "$JSONAI delete --pointer /src~1lib/hooks $TEMP_FILE --dry-run" \
    '"keep_me"'

# Actually delete the key
$JSONAI delete --pointer /src~1lib/hooks $TEMP_FILE > /dev/null 2>&1

# Verify deletion - the key should be gone
if $JSONAI query --filter '.["src/lib"].hooks' "$TEMP_FILE" 2>&1 | grep -q 'null'; then
    echo -e "Testing: DELETE: Verify key with slash was deleted... ${GREEN}PASS${NC}"
else
    echo -e "Testing: DELETE: Verify key with slash was deleted... ${RED}FAIL${NC}"
    echo "  Expected key to be deleted (null), but got:"
    $JSONAI query --filter '.["src/lib"].hooks' "$TEMP_FILE" 2>&1 | sed 's/^/    /'
    FAILURES=$((FAILURES + 1))
fi

# Test 9: Delete key with tilde
cat > "$TEMP_FILE" << 'EOF'
{
  "config~backup": "delete_me",
  "keep": "this"
}
EOF

run_test \
    "DELETE: Delete key with tilde" \
    "$JSONAI delete --pointer /config~0backup $TEMP_FILE --dry-run" \
    '"keep"'

# Actually delete the key
$JSONAI delete --pointer /config~0backup $TEMP_FILE > /dev/null 2>&1

# Verify deletion
if $JSONAI query --filter '.["config~backup"]' "$TEMP_FILE" 2>&1 | grep -q 'null'; then
    echo -e "Testing: DELETE: Verify key with tilde was deleted... ${GREEN}PASS${NC}"
else
    echo -e "Testing: DELETE: Verify key with tilde was deleted... ${RED}FAIL${NC}"
    echo "  Expected key to be deleted (null), but got:"
    $JSONAI query --filter '.["config~backup"]' "$TEMP_FILE" 2>&1 | sed 's/^/    /'
    FAILURES=$((FAILURES + 1))
fi

# Test 10: Delete key with mixed escapes
cat > "$TEMP_FILE" << 'EOF'
{
  "path/to/file~name": "delete_me",
  "keep": "this"
}
EOF

run_test \
    "DELETE: Delete key with mixed escapes" \
    "$JSONAI delete --pointer /path~1to~1file~0name $TEMP_FILE --dry-run" \
    '"keep"'

# Actually delete the key
$JSONAI delete --pointer /path~1to~1file~0name $TEMP_FILE > /dev/null 2>&1

# Verify deletion
if $JSONAI query --filter '.["path/to/file~name"]' "$TEMP_FILE" 2>&1 | grep -q 'null'; then
    echo -e "Testing: DELETE: Verify key with mixed escapes was deleted... ${GREEN}PASS${NC}"
else
    echo -e "Testing: DELETE: Verify key with mixed escapes was deleted... ${RED}FAIL${NC}"
    echo "  Expected key to be deleted (null), but got:"
    $JSONAI query --filter '.["path/to/file~name"]' "$TEMP_FILE" 2>&1 | sed 's/^/    /'
    FAILURES=$((FAILURES + 1))
fi

# Test 11: Complex scenario - nested operations
cat > "$TEMP_FILE" << 'EOF'
{
  "src/lib": {
    "sub~dir/nested": {
      "value": 1
    }
  }
}
EOF

run_test \
    "COMPLEX: Set nested value with multiple levels of escapes" \
    "$JSONAI set --pointer /src~1lib/sub~0dir~1nested/value '2' $TEMP_FILE --dry-run" \
    '2'

$JSONAI set --pointer /src~1lib/sub~0dir~1nested/value '2' $TEMP_FILE > /dev/null 2>&1

run_test \
    "COMPLEX: Add to nested structure with escapes" \
    "$JSONAI add --pointer /src~1lib/sub~0dir~1nested/new~1key '\"added\"' $TEMP_FILE --dry-run" \
    '"added"'

$JSONAI add --pointer /src~1lib/sub~0dir~1nested/new~1key '"added"' "$TEMP_FILE" > /dev/null 2>&1 || true

run_test \
    "COMPLEX: Delete from nested structure with escapes" \
    "$JSONAI delete --pointer /src~1lib/sub~0dir~1nested/value $TEMP_FILE --dry-run" \
    '"added"'

$JSONAI delete --pointer /src~1lib/sub~0dir~1nested/value $TEMP_FILE > /dev/null 2>&1

# Verify final state doesn't contain the deleted key
if $JSONAI query --filter '.["src/lib"]["sub~dir/nested"].value' "$TEMP_FILE" 2>&1 | grep -q 'null'; then
    echo -e "Testing: COMPLEX: Verify nested deletion worked... ${GREEN}PASS${NC}"
else
    echo -e "Testing: COMPLEX: Verify nested deletion worked... ${RED}FAIL${NC}"
    echo "  Expected nested value to be deleted (null), but got:"
    $JSONAI query --filter '.["src/lib"]["sub~dir/nested"].value' "$TEMP_FILE" 2>&1 | sed 's/^/    /'
    FAILURES=$((FAILURES + 1))
fi

# Test 12: Array operations with keys containing slashes (parent context)
cat > "$TEMP_FILE" << 'EOF'
{
  "src/lib": {
    "items": ["a", "b", "c"]
  }
}
EOF

run_test \
    "ARRAY: Set array element in parent with slash" \
    "$JSONAI set --pointer /src~1lib/items/0 '\"x\"' $TEMP_FILE --dry-run" \
    '"x"'

$JSONAI set --pointer /src~1lib/items/0 '"x"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Verify array was updated
run_test \
    "ARRAY: Verify array update with slash parent" \
    "$JSONAI query --filter '.[\"src/lib\"].items[0]' $TEMP_FILE" \
    'x'

run_test \
    "ARRAY: Add to array with slash parent" \
    "$JSONAI add --pointer /src~1lib/items/- '\"d\"' $TEMP_FILE --dry-run" \
    '"d"'

$JSONAI add --pointer /src~1lib/items/- '"d"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Verify array length became 4
array_length=$($JSONAI query --filter '.["src/lib"].items | length' "$TEMP_FILE" 2>/dev/null | tr -d ' ')
if [ "$array_length" = "4" ]; then
    echo -e "Testing: ARRAY: Verify array add increased length... ${GREEN}PASS${NC}"
else
    echo -e "Testing: ARRAY: Verify array add increased length... ${RED}FAIL${NC}"
    echo "  Expected length 4, got $array_length"
    FAILURES=$((FAILURES + 1))
fi

run_test \
    "ARRAY: Delete from array with slash parent" \
    "$JSONAI delete --pointer /src~1lib/items/0 $TEMP_FILE --dry-run" \
    '"b"'

# Test 13: Edge case - key is just a tilde
cat > "$TEMP_FILE" << 'EOF'
{
  "~": "tilde_key"
}
EOF

run_test \
    "EDGE: Set value with tilde-only key" \
    "$JSONAI set --pointer /~0 '\"updated\"' $TEMP_FILE --dry-run" \
    '"updated"'

# Test 14: Edge case - key is just a slash (encoded as ~1)
cat > "$TEMP_FILE" << 'EOF'
{}
EOF

run_test \
    "EDGE: Add key with slash-only" \
    "$JSONAI add --pointer /~1 '\"slash_key\"' $TEMP_FILE --dry-run" \
    '"slash_key"'

$JSONAI add --pointer /~1 '"slash_key"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Verify we can query it back
run_test \
    "EDGE: Verify slash-only key was added" \
    "$JSONAI query --filter '.[\"/\"]' $TEMP_FILE" \
    'slash_key'

# Test 15: Edge case - multiple consecutive slashes
cat > "$TEMP_FILE" << 'EOF'
{}
EOF

run_test \
    "EDGE: Add key with multiple consecutive slashes" \
    "$JSONAI add --pointer /path~1~1to '\"consecutive_slashes\"' $TEMP_FILE --dry-run" \
    '"consecutive_slashes"'

$JSONAI add --pointer /path~1~1to '"consecutive_slashes"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Verify
run_test \
    "EDGE: Verify consecutive slashes key was added" \
    "$JSONAI query --filter '.[\"path//to\"]' $TEMP_FILE" \
    'consecutive_slashes'

# Test 16: Edge case - multiple consecutive tildes
cat > "$TEMP_FILE" << 'EOF'
{}
EOF

run_test \
    "EDGE: Add key with multiple consecutive tildes" \
    "$JSONAI add --pointer /a~0~0b '\"consecutive_tildes\"' $TEMP_FILE --dry-run" \
    '"consecutive_tildes"'

$JSONAI add --pointer /a~0~0b '"consecutive_tildes"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Verify
run_test \
    "EDGE: Verify consecutive tildes key was added" \
    "$JSONAI query --filter '.[\"a~~b\"]' $TEMP_FILE" \
    'consecutive_tildes'

# Test 17: Real-world scenario - file paths as keys
cat > "$TEMP_FILE" << 'EOF'
{
  "files": {}
}
EOF

# Add several file paths
$JSONAI add --pointer /files/src~1lib~1utils.ts '"module1"' $TEMP_FILE > /dev/null 2>&1
$JSONAI add --pointer /files/src~1components~1Button.tsx '"module2"' $TEMP_FILE > /dev/null 2>&1
$JSONAI add --pointer /files/README~0backup.md '"backup"' $TEMP_FILE > /dev/null 2>&1

run_test \
    "REAL-WORLD: Query file path with slashes" \
    "$JSONAI query --filter '.files[\"src/lib/utils.ts\"]' $TEMP_FILE" \
    'module1'

run_test \
    "REAL-WORLD: Query another file path with slashes" \
    "$JSONAI query --filter '.files[\"src/components/Button.tsx\"]' $TEMP_FILE" \
    'module2'

run_test \
    "REAL-WORLD: Query file path with tilde" \
    "$JSONAI query --filter '.files[\"README~backup.md\"]' $TEMP_FILE" \
    'backup'

# Update one
run_test \
    "REAL-WORLD: Update file path entry" \
    "$JSONAI set --pointer /files/src~1lib~1utils.ts '\"updated_module\"' $TEMP_FILE --dry-run" \
    '"updated_module"'

$JSONAI set --pointer /files/src~1lib~1utils.ts '"updated_module"' "$TEMP_FILE" > /dev/null 2>&1 || true

# Delete one
$JSONAI delete --pointer /files/src~1components~1Button.tsx $TEMP_FILE > /dev/null 2>&1
if $JSONAI query --filter '.files["src/components/Button.tsx"]' "$TEMP_FILE" 2>&1 | grep -q 'null'; then
    echo -e "Testing: REAL-WORLD: Verify file path was deleted... ${GREEN}PASS${NC}"
else
    echo -e "Testing: REAL-WORLD: Verify file path was deleted... ${RED}FAIL${NC}"
    echo "  Expected file path to be deleted (null), but got:"
    $JSONAI query --filter '.files["src/components/Button.tsx"]' "$TEMP_FILE" 2>&1 | sed 's/^/    /'
    FAILURES=$((FAILURES + 1))
fi

echo ""
echo "=========================================="
echo "Test Summary"
echo "=========================================="

if [ $FAILURES -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}$FAILURES test(s) failed${NC}"
    exit 1
fi
