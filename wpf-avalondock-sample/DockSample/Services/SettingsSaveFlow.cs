namespace DockSample.Services;

/// <summary>
/// 「保存先が決まっていなければ聞いてから保存する」という流れ。
/// 設定ペインの「保存」「名前を付けて保存」と、終了時の確認の両方で使う。
/// </summary>
public static class SettingsSaveFlow
{
    /// <summary>
    /// 保存先フォルダが決まっていなければ（または askPath が true なら）フォルダを聞いてから保存する。
    /// 選んだフォルダに既に設定ファイルがあれば、上書きしてよいか確認する。
    /// 保存できたら true。キャンセル・失敗なら false（失敗時はエラーを表示する）。
    /// </summary>
    public static bool SaveInteractive(this ISettingsService settings, IDialogService dialogs, bool askPath = false)
    {
        if (!askPath && settings.FolderPath is not null)
            return ReportIfFailed(settings.Save(), settings.FolderPath, dialogs);

        var folder = dialogs.ShowSelectFolder(
            "設定の保存先フォルダを選択（中に設定ファイルを作成します）", settings.FolderPath);
        if (folder is null) return false;   // キャンセル

        // 今のフォルダ以外で、既に設定ファイルがあるなら上書き確認
        var isCurrent = string.Equals(folder, settings.FolderPath, StringComparison.OrdinalIgnoreCase);
        if (!isCurrent && settings.HasSettingsFiles(folder) &&
            !dialogs.Confirm($"このフォルダには既に設定ファイルがあります。上書きしますか？\n{folder}"))
            return false;

        return ReportIfFailed(settings.SaveAs(folder), folder, dialogs);
    }

    private static bool ReportIfFailed(bool ok, string folder, IDialogService dialogs)
    {
        if (!ok) dialogs.ShowError($"設定を保存できませんでした。\n{folder}");
        return ok;
    }
}
