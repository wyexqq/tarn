param([int]$ItemNumber, [string]$ProcessName = "tarn", [int]$TimeoutMs = 5000)

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class NativeMenu {
    public delegate bool WindowVisitor(IntPtr window, IntPtr unused);
    [DllImport("user32.dll")] public static extern bool EnumWindows(WindowVisitor visitor, IntPtr unused);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr window, StringBuilder name, int size);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    const string MenuWindowClass = "#32768";
    const uint KeyDownMessage = 0x0100;
    const int DownArrowKey = 0x28;
    const int EnterKey = 0x0D;

    public static IntPtr FindVisibleMenu(uint[] processIds) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, unused) => {
            StringBuilder name = new StringBuilder(16);
            GetClassName(window, name, name.Capacity);
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (name.ToString() == MenuWindowClass && IsWindowVisible(window) && Array.IndexOf(processIds, owner) >= 0) {
                found = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static void ChooseItem(IntPtr menu, int itemNumber) {
        for (int step = 0; step < itemNumber; step++) {
            PostMessage(menu, KeyDownMessage, (IntPtr)DownArrowKey, IntPtr.Zero);
        }
        PostMessage(menu, KeyDownMessage, (IntPtr)EnterKey, IntPtr.Zero);
    }
}
"@

$deadline = (Get-Date).AddMilliseconds($TimeoutMs)
while ((Get-Date) -lt $deadline) {
  $processIds = [uint32[]]@(Get-Process $ProcessName -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $menu = [NativeMenu]::FindVisibleMenu($processIds)
  if ($menu -ne [IntPtr]::Zero) {
    [Threading.Thread]::Sleep(150)
    [NativeMenu]::ChooseItem($menu, $ItemNumber)
    exit 0
  }
  [Threading.Thread]::Sleep(100)
}
exit 1
