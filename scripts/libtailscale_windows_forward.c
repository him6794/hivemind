#include <stddef.h>

extern int TsnetListenForward(int sd, char* network, char* tailnetAddr, char* localAddr);

int tailscale_listen_forward(int sd, const char* network, const char* tailnetAddr, const char* localAddr) {
    return TsnetListenForward(sd, (char*)network, (char*)tailnetAddr, (char*)localAddr);
}
