#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "${1:-.}" && pwd)
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/pixstride-abi.XXXXXX")
trap 'rm -r "$tmp"' EXIT HUP INT TERM
transport="$repo/src/network/fetch/transport.rs"

# Fail closed if the source shape or number of libcurl options changes.
awk '
function emit(value) {
    gsub(/[[:space:]]/, "", value)
    if (value !~ /^(0[xX][[:xdigit:]]+|[0-9]+)$/) { failed = 1; exit 1 }
    print value ","; count++
}
BEGIN { print "[" }
/^    fn one\(/ { inside = 1; next }
inside && /^    fn fetch\(/ { finished = 1; exit }
inside && /self\.long\(/ {
    value = $0; sub(/.*self\.long\(/, "", value); sub(/,.*/, "", value); emit(value)
}
inside && /curl_easy_setopt\(/ {
    value = $0
    if (value ~ /curl_easy_setopt\(self\.0,/) {
        sub(/.*curl_easy_setopt\(self\.0,/, "", value); sub(/,.*/, "", value)
    } else {
        if ((getline line) <= 0 || line !~ /self\.0,/) { failed = 1; exit 1 }
        if ((getline value) <= 0) { failed = 1; exit 1 }
        sub(/,.*/, "", value)
    }
    emit(value)
}
END {
    if (failed || !finished || count != 23) {
        print "could not extract all curl options from transport.rs" > "/dev/stderr"; exit 1
    }
    print "]"
}' "$transport" > "$tmp/curl_options.rs"

awk '
function emit(value) {
    gsub(/[[:space:]]/, "", value)
    if (value !~ /^(0[xX][[:xdigit:]]+|[0-9]+)$/) { failed = 1; exit 1 }
    print value ","; count++
}
BEGIN { print "[" }
/^    fn one\(/ { inside = 1; next }
inside && /^    fn fetch\(/ { finished = 1; exit }
inside && /self\.info\(/ {
    value = $0; sub(/.*self\.info\(/, "", value); sub(/\).*/, "", value); emit(value)
}
inside && /curl_easy_getinfo\(self\.0,/ {
    value = $0; sub(/.*curl_easy_getinfo\(self\.0,/, "", value); sub(/,.*/, "", value); emit(value)
}
END {
    if (failed || !finished || count != 3) {
        print "could not extract all curl info IDs from transport.rs" > "/dev/stderr"; exit 1
    }
    print "]"
}' "$transport" > "$tmp/curl_infos.rs"

awk '
function emit(value) {
    gsub(/[[:space:]]/, "", value)
    if (value !~ /^[()0-9<>]+$/) { failed = 1; exit 1 }
    print value ","; count++
}
BEGIN { print "[" }
/curl_version_info\([0-9]/ {
    value = $0; sub(/.*curl_version_info\(/, "", value); sub(/\).*/, "", value); emit(value)
}
/let thread_safe =/ {
    value = $0; sub(/.* & /, "", value); sub(/ !=.*/, "", value); emit(value)
}
/curl_global_init\([0-9]/ {
    value = $0; sub(/.*curl_global_init\(/, "", value); sub(/\).*/, "", value); emit(value)
}
END {
    if (failed || count != 3) {
        print "could not extract curl initialization values from transport.rs" > "/dev/stderr"; exit 1
    }
    print "]"
}' "$transport" > "$tmp/curl_init_values.rs"

awk '
/#\[cfg\(target_os = "linux"\)\]/ { platform = "linux"; next }
/#\[cfg\(target_os = "macos"\)\]/ { platform = "macos"; next }
platform != "" && /let socket_flags =/ {
    value = $0; sub(/.*let socket_flags =/, "", value); sub(/;.*/, "", value)
    if (value !~ /^[[:space:]0-9a-fA-FxX|]+$/ || seen[platform]++) { failed = 1; exit 1 }
    print "#[cfg(target_os = \"" platform "\")]"
    print "const SOCKET_FLAGS: i32 = " value ";"
    count++; platform = ""; next
}
{ platform = "" }
END {
    if (failed || count != 2) {
        print "could not extract socket flags from transport.rs" > "/dev/stderr"; exit 1
    }
}' "$transport" > "$tmp/curl_socket_flags.rs"

awk '
BEGIN { print "[" }
/Ipv4Addr::new/ {
    rest = $0
    while (match(rest, /a\.address\[[0-9]+\]/)) {
        value = substr(rest, RSTART, RLENGTH)
        rest = substr(rest, RSTART + RLENGTH)
        sub(/a\.address\[/, "", value); sub(/\].*/, "", value)
        print value ","; count++
    }
}
END {
    if (failed || count != 4) {
        print "could not extract IPv4 octets from transport.rs" > "/dev/stderr"; exit 1
    }
    print "]"
}' "$transport" > "$tmp/ipv4_octets.rs"

cat "$transport" > "$tmp/transport_abi_probe.rs"
printf '\ntransport_abi_checks!();\n' >> "$tmp/transport_abi_probe.rs"
cat "$repo/src/network/local.rs" > "$tmp/local_abi_probe.rs"
printf '\nlocal_abi_checks!();\n' >> "$tmp/local_abi_probe.rs"
cp "$here/abi_harness.rs" "$here/abi_probe_tests.rs" "$tmp/"
cp "$repo/src/network/fetch/policy.rs" "$tmp/policy.rs"
cp "$repo/src/network/fetch/response.rs" "$tmp/response.rs"
cc -std=c11 -Wall -Wextra -Werror -c "$here/abi_check.c" -o "$tmp/abi_check.o"
# RUSTFLAGS carries the build target and linker options where provided.
# shellcheck disable=SC2086
CARGO_PKG_VERSION=abi rustc --edition=2021 "$tmp/abi_harness.rs" ${RUSTFLAGS:-} -C "link-arg=$tmp/abi_check.o" -o "$tmp/abi_check"
"$tmp/abi_check"
