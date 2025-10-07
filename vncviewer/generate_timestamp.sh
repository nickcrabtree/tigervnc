#!/bin/sh
# Generate build timestamp header file

OUTPUT_FILE="$1"
TEMP_FILE="${OUTPUT_FILE}.tmp"

# Generate timestamp
TIMESTAMP=$(date -u +"%Y-%m-%d %H:%M")

# Write to temp file
echo "#define BUILD_TIMESTAMP \"${TIMESTAMP}\"" > "${TEMP_FILE}"

# Only update if changed (to avoid unnecessary recompilation)
if ! cmp -s "${TEMP_FILE}" "${OUTPUT_FILE}" 2>/dev/null; then
    mv "${TEMP_FILE}" "${OUTPUT_FILE}"
else
    rm -f "${TEMP_FILE}"
fi
