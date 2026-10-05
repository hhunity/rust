namespace DockSample.Settings;

/// <summary>
/// 2つのオブジェクトの間で「同じ名前・代入できる型」のプロパティをコピーする。
/// ViewModel ⇄ 設定クラス の書き写しを、プロパティごとに書かずに済ませるための道具。
/// </summary>
public static class PropertyCopier
{
    /// <summary>to の全プロパティについて、from に同じ名前があればコピーする</summary>
    public static void CopyAll(object from, object to)
    {
        foreach (var p in to.GetType().GetProperties())
            CopyOne(from, to, p.Name);
    }

    /// <summary>名前を指定して1つだけコピーする（どちらかにその名前がなければ何もしない）</summary>
    public static void CopyOne(object from, object to, string? name)
    {
        if (string.IsNullOrEmpty(name)) return;

        var src = from.GetType().GetProperty(name);
        var dst = to.GetType().GetProperty(name);
        if (src is { CanRead: true } && dst is { CanWrite: true }
            && dst.SetMethod is { IsPublic: true }
            && dst.PropertyType.IsAssignableFrom(src.PropertyType))
        {
            dst.SetValue(to, src.GetValue(from));
        }
    }
}
