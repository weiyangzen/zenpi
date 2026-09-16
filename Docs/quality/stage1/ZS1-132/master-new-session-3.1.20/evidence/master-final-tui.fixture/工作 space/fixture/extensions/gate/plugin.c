
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>
int main(void) {
char line[65536];
if (!fgets(line, sizeof(line), stdin)) return 2;
int initialize = strstr(line, "\"method\":\"initialize\"") != NULL;
char *id = strstr(line, "\"id\":");
char *cap = strstr(line, "\"capability\":{");
if (!id || !cap) return 3;
unsigned long request = strtoul(id + 5, NULL, 10);
cap += 13;
char *end = strchr(cap, '}');
if (!end) return 4;
end[1] = 0;
if (initialize && access("block", F_OK) == 0) {
    FILE *marker = fopen("entered", "w");
    if (!marker) return 5;
    fprintf(marker, "%ld", (long)getpid()); fclose(marker);
    while (access("release", F_OK) != 0) usleep(2000);
}
printf("{\"jsonrpc\":\"2.0\",\"id\":%lu,\"capability\":%s,\"result\":%s}\n", request, cap,
    initialize ? "{\"api_version\":2,\"hooks\":[\"session_start\"],\"tools\":[]}" : "{\"action\":\"continue\"}");
return 0;
}
