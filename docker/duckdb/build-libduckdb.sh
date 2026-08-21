#!/usr/bin/env bash
# Builds the pinned DuckDB C API from source for a native Debian/glibc Linux target.
set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly BUILD_ROOT=/tmp/duckdb-build
readonly INSTALL_ROOT=/opt/duckdb
readonly ARTIFACT_ROOT=/opt/duckdb-artifacts

# shellcheck source=duckdb-build.env
source "${SCRIPT_DIR}/duckdb-build.env"

require_supported_target() {
    if [[ "${TARGETOS:-}" != linux ]]; then
        printf 'DuckDB builder supports only Linux glibc targets; got TARGETOS=%s\n' "${TARGETOS:-unset}" >&2
        exit 64
    fi

    case "${TARGETARCH:-}" in
        amd64) expected_machine='Advanced Micro Devices X86-64' ;;
        arm64) expected_machine='AArch64' ;;
        *)
            printf 'DuckDB builder supports only TARGETARCH=amd64 or arm64; got %s\n' "${TARGETARCH:-unset}" >&2
            exit 64
            ;;
    esac

    if [[ "${TARGETPLATFORM:-linux/${TARGETARCH}}" != "linux/${TARGETARCH}" ]]; then
        printf 'DuckDB builder requires TARGETPLATFORM=linux/%s; got %s\n' \
            "${TARGETARCH}" "${TARGETPLATFORM}" >&2
        exit 64
    fi

    if ! getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
        printf 'DuckDB builder requires glibc; musl and other C libraries are unsupported\n' >&2
        exit 64
    fi
}

verify_source_commit_when_present() {
    local source_dir=$1
    local archive_metadata

    if [[ -d "${source_dir}/.git" ]]; then
        command -v git >/dev/null || { printf 'source has .git metadata but git is unavailable\n' >&2; exit 1; }
        [[ "$(git -C "${source_dir}" rev-parse HEAD)" == "${DUCKDB_SOURCE_COMMIT}" ]] || {
            printf 'DuckDB source commit does not match pinned metadata\n' >&2
            exit 1
        }
        return
    fi

    archive_metadata="${source_dir}/.git_archival.txt"
    if [[ -f "${archive_metadata}" ]]; then
        grep -Fqx "node: ${DUCKDB_SOURCE_COMMIT}" "${archive_metadata}" || {
            printf 'DuckDB archival source commit does not match pinned metadata\n' >&2
            exit 1
        }
    fi
}

require_supported_target

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
    binutils build-essential ca-certificates cmake curl file ninja-build unzip
rm -rf /var/lib/apt/lists/*

rm -rf "${BUILD_ROOT}" "${INSTALL_ROOT}" "${ARTIFACT_ROOT}"
install -d -m 0755 "${BUILD_ROOT}" "${INSTALL_ROOT}" "${ARTIFACT_ROOT}"

curl --fail --location --retry 3 --retry-all-errors --connect-timeout 20 --max-time 600 \
    --output "${BUILD_ROOT}/libduckdb-src.zip" "${DUCKDB_SOURCE_URL}"
printf '%s  %s\n' "${DUCKDB_SOURCE_SHA256}" "${BUILD_ROOT}/libduckdb-src.zip" | sha256sum --check --status

readonly SOURCE_DIR="${BUILD_ROOT}/source"
install -d -m 0755 "${SOURCE_DIR}"
unzip -q "${BUILD_ROOT}/libduckdb-src.zip" -d "${SOURCE_DIR}"
[[ -f "${SOURCE_DIR}/duckdb.cpp" && -f "${SOURCE_DIR}/duckdb.h" ]] || {
    printf 'DuckDB source archive lacks the expected C API amalgamation\n' >&2
    exit 1
}
verify_source_commit_when_present "${SOURCE_DIR}"

cat > "${BUILD_ROOT}/CMakeLists.txt" <<'EOF'
cmake_minimum_required(VERSION 3.16)
project(duckdb_shared LANGUAGES CXX)

set(NATIVE_ARCH OFF CACHE BOOL "Disable host CPU tuning")
set(BUILD_SHELL OFF CACHE BOOL "Do not build the DuckDB shell")
set(BUILD_TESTING OFF CACHE BOOL "Do not build tests")
set(BUILD_UNITTESTS OFF CACHE BOOL "Do not build unit tests")
add_library(duckdb SHARED "${DUCKDB_SOURCE_DIR}/duckdb.cpp")
target_include_directories(duckdb PUBLIC "${DUCKDB_SOURCE_DIR}")
target_compile_features(duckdb PRIVATE cxx_std_11)
install(TARGETS duckdb LIBRARY DESTINATION lib)
install(FILES "${DUCKDB_SOURCE_DIR}/duckdb.h" "${DUCKDB_SOURCE_DIR}/duckdb_extension.h" DESTINATION include)
EOF

cmake -S "${BUILD_ROOT}" -B "${BUILD_ROOT}/cmake" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="${INSTALL_ROOT}" \
    -DDUCKDB_SOURCE_DIR="${SOURCE_DIR}" \
    -DBUILD_SHARED_LIBS=ON \
    -DNATIVE_ARCH=OFF \
    -DBUILD_SHELL=OFF \
    -DBUILD_TESTING=OFF \
    -DBUILD_UNITTESTS=OFF
cmake --build "${BUILD_ROOT}/cmake" --target duckdb --parallel 1
cmake --install "${BUILD_ROOT}/cmake"

readonly LIBRARY_PATH="${INSTALL_ROOT}/lib/libduckdb.so"
[[ -s "${LIBRARY_PATH}" ]] || { printf 'CMake did not install libduckdb.so\n' >&2; exit 1; }
actual_machine="$(readelf --file-header "${LIBRARY_PATH}" | awk -F: '/Machine:/{gsub(/^[[:space:]]+/, "", $2); print $2}')"
[[ "${actual_machine}" == "${expected_machine}" ]] || {
    printf 'libduckdb.so machine mismatch: expected %s for linux/%s, got %s\n' \
        "${expected_machine}" "${TARGETARCH}" "${actual_machine:-unknown}" >&2
    exit 1
}
gcc -std=c11 -Wall -Wextra -Werror "${SCRIPT_DIR}/duckdb-smoke.c" \
    -I "${INSTALL_ROOT}/include" -L "${INSTALL_ROOT}/lib" -Wl,-rpath,'$ORIGIN/../lib' -lduckdb \
    -o "${BUILD_ROOT}/duckdb-smoke"
install -D -m 0755 "${BUILD_ROOT}/duckdb-smoke" "${INSTALL_ROOT}/libexec/duckdb-smoke"
"${INSTALL_ROOT}/libexec/duckdb-smoke"
ldd "${LIBRARY_PATH}" | tee "${ARTIFACT_ROOT}/libduckdb.ldd"
! grep -Fq 'not found' "${ARTIFACT_ROOT}/libduckdb.ldd"

sha256sum "${LIBRARY_PATH}" | tee "${ARTIFACT_ROOT}/libduckdb.so.sha256"
cat > "${ARTIFACT_ROOT}/libduckdb.provenance.json" <<EOF
{
  "duckdb_version": "${DUCKDB_VERSION}",
  "source_commit": "${DUCKDB_SOURCE_COMMIT}",
  "source_url": "${DUCKDB_SOURCE_URL}",
  "source_sha256": "${DUCKDB_SOURCE_SHA256}",
  "target_os": "linux",
  "target_arch": "${TARGETARCH}",
  "elf_machine": "${actual_machine}",
  "libduckdb_sha256": "$(cut -d ' ' -f 1 "${ARTIFACT_ROOT}/libduckdb.so.sha256")"
}
EOF
