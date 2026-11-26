// Win32 driver that runs ATMISP's GUI to generate an SVF file (oracle for testing).
// usage: drive.exe <chain.chn> <out.svf> <C|D> <statereset 0|1> <tck_us>
#include <windows.h>
#include <stdio.h>
#include <string.h>

static DWORD g_pid;
static HWND g_main;

static BOOL CALLBACK find_main(HWND h, LPARAM lp) {
    DWORD pid; GetWindowThreadProcessId(h, &pid);
    if (pid != g_pid || !IsWindowVisible(h)) return TRUE;
    // main frame contains the form view, which has control 1007 (Run) somewhere below
    HWND run = NULL;
    HWND c = GetWindow(h, GW_CHILD);
    while (c) {
        if (GetDlgItem(c, 1007)) { run = GetDlgItem(c, 1007); break; }
        HWND cc = GetWindow(c, GW_CHILD);
        while (cc) { if (GetDlgItem(cc, 1007)) { run = GetDlgItem(cc, 1007); break; } cc = GetWindow(cc, GW_HWNDNEXT); }
        if (run) break;
        c = GetWindow(c, GW_HWNDNEXT);
    }
    if (run) { g_main = GetParent(run); return FALSE; }
    return TRUE;
}

static char g_msg[4096];
static int g_nmsg;
static HWND g_box;

static BOOL CALLBACK find_box(HWND h, LPARAM lp) {
    DWORD pid; GetWindowThreadProcessId(h, &pid);
    if (pid != g_pid || !IsWindowVisible(h)) return TRUE;
    char cls[64]; GetClassNameA(h, cls, sizeof cls);
    if (strcmp(cls, "#32770") == 0 && GetDlgItem(h, 0xFFFF) != NULL) { g_box = h; return FALSE; }
    return TRUE;
}

static BOOL CALLBACK collect_text(HWND h, LPARAM lp) {
    char cls[64], t[2048]; GetClassNameA(h, cls, sizeof cls);
    if (strcmp(cls, "Static") == 0) {
        { WCHAR w[2048]; GetWindowTextW(h, w, 2048); WideCharToMultiByte(CP_UTF8, 0, w, -1, t, sizeof t, NULL, NULL); }
        if (t[0]) { strncat(g_msg, t, sizeof g_msg - strlen(g_msg) - 2); strcat(g_msg, " "); }
    }
    return TRUE;
}

// Dismiss any message box; returns 1 if one was handled. Text is printed.
static int pump_boxes(int answer_yes) {
    g_box = NULL;
    EnumWindows(find_box, 0);
    if (!g_box) return 0;
    g_msg[0] = 0;
    EnumChildWindows(g_box, collect_text, 0);
    for (char *p = g_msg; *p; p++) if (*p == '\n' || *p == '\r') *p = ' ';
    printf("MSGBOX: %s\n", g_msg); fflush(stdout);
    g_nmsg++;
    int id = IDOK;
    if (GetDlgItem(g_box, IDYES)) id = answer_yes ? IDYES : IDNO;
    else if (!GetDlgItem(g_box, IDOK) && GetDlgItem(g_box, IDCANCEL)) id = IDCANCEL;
    PostMessageA(g_box, WM_COMMAND, MAKEWPARAM(id, BN_CLICKED), (LPARAM)GetDlgItem(g_box, id));
    Sleep(300);
    return 1;
}

static void click(HWND dlg, int id) {
    HWND b = GetDlgItem(dlg, id);
    SendMessageA(dlg, WM_COMMAND, MAKEWPARAM(id, BN_CLICKED), (LPARAM)b);
}

int main(int argc, char **argv) {
    if (argc < 6) { fprintf(stderr, "usage: drive chn svf C|D statereset tck [exe]\n"); return 2; }
    { char lp[1100]; snprintf(lp, sizeof lp, "%s.log", argv[2]); freopen(lp, "w", stdout); }
    const char *exe = argc > 6 ? argv[6] : "C:\\ATMISP7\\ATMISP.exe";
    char cmd[1024];
    snprintf(cmd, sizeof cmd, "\"%s\" \"%s\"", exe, argv[1]);
    STARTUPINFOA si = {sizeof si}; PROCESS_INFORMATION pi;
    if (!CreateProcessA(NULL, cmd, NULL, NULL, FALSE, 0, NULL, "C:\\ATMISP7", &si, &pi)) { printf("ERROR: CreateProcess %lu\n", GetLastError()); return 1; }
    g_pid = pi.dwProcessId;
    WaitForInputIdle(pi.hProcess, 10000);

    for (int i = 0; i < 200 && !g_main; i++) {
        while (pump_boxes(0)) {}
        EnumWindows(find_main, 0);
        if (!g_main) Sleep(100);
    }
    if (!g_main) { printf("ERROR: main window not found\n"); TerminateProcess(pi.hProcess, 1); return 1; }
    Sleep(500);
    while (pump_boxes(0)) {}

    // SVF checkbox
    HWND chk = GetDlgItem(g_main, 1020);
    if (SendMessageA(chk, BM_GETCHECK, 0, 0) != BST_CHECKED) { SendMessageA(chk, BM_SETCHECK, BST_CHECKED, 0); click(g_main, 1020); }
    while (pump_boxes(0)) {}
    SetDlgItemTextA(g_main, 1019, argv[2]);
    // version combo: find item text "Revision C"/"Revision D"
    HWND cb = GetDlgItem(g_main, 1022);
    int n = SendMessageA(cb, CB_GETCOUNT, 0, 0), sel = -1;
    for (int i = 0; i < n; i++) { char t[64]; SendMessageA(cb, CB_GETLBTEXT, i, (LPARAM)t); printf("COMBO[%d]='%s'\n", i, t); }
    sel = (argv[3][0] == 'D') ? 1 : 0;
    if (sel < 0) { printf("ERROR: revision not found (%d items)\n", n); }
    SendMessageA(cb, CB_SETCURSEL, sel, 0);
    SendMessageA(g_main, WM_COMMAND, MAKEWPARAM(1022, CBN_SELCHANGE), (LPARAM)cb);
    while (pump_boxes(0)) {}
    HWND sr = GetDlgItem(g_main, 1024);
    int want = atoi(argv[4]) ? BST_CHECKED : BST_UNCHECKED;
    if (SendMessageA(sr, BM_GETCHECK, 0, 0) != want) { SendMessageA(sr, BM_SETCHECK, want, 0); click(g_main, 1024); }
    SetDlgItemTextA(g_main, 1023, argv[5]);
    SendMessageA(g_main, WM_COMMAND, MAKEWPARAM(1023, EN_CHANGE), (LPARAM)GetDlgItem(g_main, 1023));
    SendMessageA(g_main, WM_COMMAND, MAKEWPARAM(1023, EN_KILLFOCUS), (LPARAM)GetDlgItem(g_main, 1023));
    while (pump_boxes(0)) {}

    char t[1024];
    GetDlgItemTextA(g_main, 1019, t, sizeof t); printf("STATE: svf='%s' chk=%ld rev=%ld sr=%ld\n", t,
        (long)SendMessageA(chk, BM_GETCHECK, 0, 0), (long)SendMessageA(cb, CB_GETCURSEL, 0, 0), (long)SendMessageA(sr, BM_GETCHECK, 0, 0));
    fflush(stdout);

    PostMessageA(g_main, WM_COMMAND, MAKEWPARAM(1007, BN_CLICKED), (LPARAM)GetDlgItem(g_main, 1007));
    // wait for completion: a message box appears at the end
    DWORD start = GetTickCount(), last_change = GetTickCount();
    LONGLONG last_size = -1;
    int got = 0;
    while (GetTickCount() - start < 90000) {
        if (pump_boxes(1)) { got = 1; last_change = GetTickCount(); continue; }
        WIN32_FILE_ATTRIBUTE_DATA fa;
        LONGLONG sz = -1;
        if (GetFileAttributesExA(argv[2], GetFileExInfoStandard, &fa)) sz = ((LONGLONG)fa.nFileSizeHigh << 32) | fa.nFileSizeLow;
        if (sz != last_size) { last_size = sz; last_change = GetTickCount(); }
        else if (GetTickCount() - last_change > 4000 && IsWindowEnabled(GetDlgItem(g_main, 1007))) { got = 2; break; }
        Sleep(100);
    }
    printf("DONE: how=%d ms=%lu size=%lld\n", got, GetTickCount() - start, last_size);
    // dump transcript listbox (wide API: ANSI text is mangled under wine)
    HWND lb = GetDlgItem(g_main, 1013);
    n = SendMessageW(lb, LB_GETCOUNT, 0, 0);
    for (int i = 0; i < n; i++) {
        WCHAR wl[2048]; char line[4096];
        int len = SendMessageW(lb, LB_GETTEXTLEN, i, 0);
        if (len >= 2047) continue;
        SendMessageW(lb, LB_GETTEXT, i, (LPARAM)wl);
        WideCharToMultiByte(CP_UTF8, 0, wl, -1, line, sizeof line, NULL, NULL);
        printf("LOG: %s\n", line);
    }
    fflush(stdout);
    TerminateProcess(pi.hProcess, 0);
    return 0;
}
