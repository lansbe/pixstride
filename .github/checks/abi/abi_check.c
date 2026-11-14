#define _GNU_SOURCE 1
#define CURL_DISABLE_DEPRECATION 1
#include <curl/curl.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <stddef.h>
#include <string.h>
#include <sys/socket.h>

_Static_assert(sizeof(curlsocktype) == sizeof(int), "curlsocktype width");
_Static_assert(sizeof(CURLversion) == sizeof(int), "CURLversion width");
_Static_assert(sizeof(CURLoption) == sizeof(int), "CURLoption width");
_Static_assert(sizeof(CURLINFO) == sizeof(int), "CURLINFO width");
_Static_assert(sizeof(CURLcode) == sizeof(int), "CURLcode width");
_Static_assert(sizeof(curl_socket_t) == sizeof(int), "curl_socket_t width");
_Static_assert(_Generic((curl_write_callback)0,
    size_t (*)(char *, size_t, size_t, void *): 1, default: 0), "write callback");
_Static_assert(_Generic((curl_opensocket_callback)0,
    curl_socket_t (*)(void *, curlsocktype, struct curl_sockaddr *): 1,
    default: 0), "open socket callback");
_Static_assert(_Generic(&openat,
    int (*)(int, const char *, int, ...): 1, default: 0), "openat signature");

#define PREFIX_END (offsetof(curl_version_info_data, features) + sizeof(int))
#define PREFIX_ALIGN _Alignof(curl_version_info_data)
static const size_t layouts[] = {
    sizeof(struct curl_sockaddr), _Alignof(struct curl_sockaddr),
    offsetof(struct curl_sockaddr, family), offsetof(struct curl_sockaddr, socktype),
    offsetof(struct curl_sockaddr, protocol), offsetof(struct curl_sockaddr, addrlen),
    offsetof(struct curl_sockaddr, addr), sizeof(struct sockaddr),
    (PREFIX_END + PREFIX_ALIGN - 1) / PREFIX_ALIGN * PREFIX_ALIGN, PREFIX_ALIGN,
    offsetof(curl_version_info_data, age), offsetof(curl_version_info_data, version),
    offsetof(curl_version_info_data, version_num), offsetof(curl_version_info_data, host),
    offsetof(curl_version_info_data, features), sizeof(struct sockaddr_in),
    sizeof(size_t), sizeof(long), sizeof(int), sizeof(unsigned int), sizeof(void *),
    sizeof(curl_off_t), _Alignof(curl_off_t)
};
static const int options[] = {
    CURLOPT_NOSIGNAL, CURLOPT_TIMEOUT_MS, CURLOPT_CONNECTTIMEOUT_MS,
    CURLOPT_PROTOCOLS, CURLOPT_FOLLOWLOCATION, CURLOPT_REDIR_PROTOCOLS,
    CURLOPT_SSL_VERIFYPEER, CURLOPT_SSL_VERIFYHOST, CURLOPT_IPRESOLVE,
    CURLOPT_MAXCONNECTS, CURLOPT_HTTP_VERSION, CURLOPT_TCP_KEEPALIVE,
    CURLOPT_PROXY, CURLOPT_URL, CURLOPT_USERAGENT, CURLOPT_ACCEPT_ENCODING, CURLOPT_COOKIE, CURLOPT_MAXFILESIZE_LARGE,
    CURLOPT_WRITEFUNCTION, CURLOPT_HEADERFUNCTION, CURLOPT_WRITEDATA,
    CURLOPT_HEADERDATA, CURLOPT_OPENSOCKETFUNCTION
};
static const int infos[] = {
    CURLINFO_RESPONSE_CODE, CURLINFO_NUM_CONNECTS, CURLINFO_SIZE_DOWNLOAD_T
};
static const int init_values[] = {
    CURLVERSION_FIRST, CURL_VERSION_THREADSAFE, CURL_GLOBAL_DEFAULT
};
static const int file_flags[] = {
    O_RDONLY, O_NONBLOCK, O_NOFOLLOW, O_DIRECTORY, O_CLOEXEC, O_CREAT,
#ifdef O_TMPFILE
    O_TMPFILE
#else
    0
#endif
};
#define COUNT(array) (sizeof(array) / sizeof((array)[0]))
size_t abi_layout_count(void) { return COUNT(layouts); }
size_t abi_layout(size_t index) { return layouts[index]; }
size_t abi_option_count(void) { return COUNT(options); }
int abi_option(size_t index) { return options[index]; }
size_t abi_info_count(void) { return COUNT(infos); }
int abi_info(size_t index) { return infos[index]; }
size_t abi_init_value_count(void) { return COUNT(init_values); }
int abi_init_value(size_t index) { return init_values[index]; }
size_t abi_file_flag_count(void) { return COUNT(file_flags); }
int abi_file_flag(size_t index) { return file_flags[index]; }
size_t abi_ipv4_octet_offset(void) { return offsetof(struct sockaddr_in, sin_addr); }
void abi_make_sockaddr(unsigned char *out, int valid_family) {
    struct sockaddr_in address = {0};
#ifdef __APPLE__
    address.sin_len = sizeof(address);
#endif
    address.sin_family = valid_family ? AF_INET : AF_UNSPEC;
    memcpy(out, &address, sizeof(address));
}
int abi_socket_flags(void) {
#ifdef __linux__
    return SOCK_CLOEXEC | SOCK_NONBLOCK;
#else
    return 0;
#endif
}
int abi_make_sockaddr_v6(unsigned char *out) {
    struct sockaddr_in6 address = {0};
#ifdef __APPLE__
    address.sin6_len = sizeof(address);
#endif
    address.sin6_family = AF_INET6;
    address.sin6_port = htons(443);
    address.sin6_addr.s6_addr[0] = 0x26;
    address.sin6_addr.s6_addr[1] = 0x06;
    address.sin6_addr.s6_addr[2] = 0x47;
    address.sin6_addr.s6_addr[3] = 0x00;
    address.sin6_addr.s6_addr[15] = 1;
    _Static_assert(sizeof(address) == 28, "IPv6 address width");
    memcpy(out, &address, sizeof(address));
    return AF_INET6;
}
