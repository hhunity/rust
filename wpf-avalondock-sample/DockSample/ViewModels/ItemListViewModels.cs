using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using DockSample.Services;
using DockSample.Settings;

namespace DockSample.ViewModels;

/// <summary>
/// 追加できる項目の種類（「追加 ▾」の一覧に並ぶもの）。
/// ToString は DropDownButton の検索に使われるので、名前と分類を返す。
/// </summary>
public record ItemType(string Name, string Category)
{
    public override string ToString() => $"{Name} {Category}";
}

/// <summary>
/// 一覧の1行。値は保存用の ItemData（Settings/ItemSettings.cs）から作り、ToData で書き戻す。
/// 値が変わると Edited を出す（一覧の ViewModel がそれを受けて設定に書き写す）。
/// </summary>
public partial class ItemRowViewModel : ObservableObject
{
    /// <summary>最後に保存した時のこの行（保存したことがない＝新しく追加した行なら null）</summary>
    private ItemData? _saved;

    public ItemRowViewModel(ItemType type, ItemData data)
    {
        Id = data.Id;
        Type = type;
        _name = data.Name;
        _notes = data.Notes;
        _enabled = data.Enabled;

        // 種類に応じた詳細設定。詳細が編集されたら、この行が編集されたことにする
        Details = ItemDetailsBase.Create(type, data.Details);
        Details.PropertyChanged += (_, e) => OnEdited("Details." + e.PropertyName);
    }

    /// <summary>値が変わった（引数は変わった項目の名前。Undo を1回分にまとめるのに使う）</summary>
    public event EventHandler<string>? Edited;

    /// <summary>行を見分ける番号（保存した行との対応に使う）</summary>
    public Guid Id { get; }

    /// <summary>種類（追加時に決まり、変えられない）</summary>
    public ItemType Type { get; }

    /// <summary>詳細設定（右側の詳細パネルに表示する。中身は種類の分類によって違う）</summary>
    public ItemDetailsBase Details { get; }

    [ObservableProperty] private string _name;
    [ObservableProperty] private string _notes;
    [ObservableProperty] private bool _enabled;

    partial void OnNameChanged(string value) => OnEdited(nameof(Name));
    partial void OnNotesChanged(string value) => OnEdited(nameof(Notes));
    partial void OnEnabledChanged(bool value) => OnEdited(nameof(Enabled));

    /// <summary>まだ保存していない、新しく追加した行か</summary>
    public bool IsNew => _saved is null;

    /// <summary>保存したときから値（詳細設定を含む）が変わっているか（元の値に戻せば false に戻る）</summary>
    public bool IsModified =>
        _saved is not null &&
        (Name != _saved.Name || Notes != _saved.Notes || Enabled != _saved.Enabled
         || Details.ToData() != _saved.Details);   // record どうしなので中身で比べられる

    /// <summary>表の「状態」列：新 = 未保存の新しい行 / ● = 保存後に編集した / 空 = 保存したまま</summary>
    public string Status => IsNew ? "新" : IsModified ? "●" : "";

    /// <summary>「保存した時の値」を差し替える（起動・保存・開く・Undo のあとに呼ばれる）</summary>
    public void SetSaved(ItemData? saved)
    {
        _saved = saved;
        NotifyStatus();
    }

    /// <summary>今の値を保存用のデータにする</summary>
    public ItemData ToData() => new()
    {
        Id = Id,
        TypeName = Type.Name,
        Category = Type.Category,
        Name = Name,
        Notes = Notes,
        Enabled = Enabled,
        Details = Details.ToData(),
    };

    private void OnEdited(string key)
    {
        Edited?.Invoke(this, key);
        NotifyStatus();
    }

    private void NotifyStatus()
    {
        OnPropertyChanged(nameof(IsNew));
        OnPropertyChanged(nameof(IsModified));
        OnPropertyChanged(nameof(Status));
    }
}

/// <summary>
/// 「一覧」ペイン。上の「追加 ▾」（DropDownButton）で種類を選ぶと、表に1行追加される。
/// 各行：名前・種類・備考・有効・状態の印・削除ボタン
/// 選んだ行の詳細は、表の右側の詳細パネルに表示・編集できる（SelectedItem をそのまま表示）
///
/// 行は設定（AppSettings.Items）の一部として、設定フォルダの items.json に保存される。
/// ・追加・削除・編集のたびに、全行を設定に書き写す（→ 設定ペインの「保存」「元に戻す」「未保存」の印がそのまま効く）
/// ・Undo・開く などで設定が差し替わったら、表を作り直す
/// </summary>
public partial class ItemListViewModel : PaneViewModel
{
    private readonly ISettingsService _settings;

    /// <summary>最後に保存した時にあった行の Id（「削除した行」の数を出すのに使う）</summary>
    private HashSet<Guid> _savedIds = new();

    public ItemListViewModel(ISettingsService settings)
        : base("Document_Items", "項目の一覧", DockLocation.Document, buttonText: "一覧")
    {
        _settings = settings;

        // 行の追加・削除や、各行の編集状態が変わったら、件数の表示を更新する
        Items.CollectionChanged += OnItemsChanged;

        // Undo・開く・保存した状態に戻す → 表を作り直す / 保存した → 各行の「新」「●」を消す
        settings.Reloaded += (_, _) => Load();
        settings.Saved += (_, _) => RefreshSaved();

        // Undo / Redo ができるかが変わったら、ボタンの有効・無効を更新する
        settings.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName is nameof(ISettingsService.CanUndo) or nameof(ISettingsService.CanRedo))
            {
                UndoCommand.NotifyCanExecuteChanged();
                RedoCommand.NotifyCanExecuteChanged();
            }
        };

        Load();
    }

    /// <summary>「追加 ▾」の一覧（数が多いので DropDownButton のスクロール・検索が効く）</summary>
    public IReadOnlyList<ItemType> ItemTypes => AllItemTypes;

    /// <summary>表の行</summary>
    public ObservableCollection<ItemRowViewModel> Items { get; } = new();

    /// <summary>表で選ばれている行（追加した行を選んだ状態にするのに使う）</summary>
    [ObservableProperty] private ItemRowViewModel? _selectedItem;

    /// <summary>表の上に出す「◯件（新規 ◯ / 編集 ◯ / 削除 ◯）」</summary>
    public string Summary
    {
        get
        {
            var changes = new List<string>();
            var added = Items.Count(i => i.IsNew);
            var modified = Items.Count(i => i.IsModified);
            var removed = _savedIds.Count(id => Items.All(i => i.Id != id));
            if (added > 0) changes.Add($"新規 {added}");
            if (modified > 0) changes.Add($"編集 {modified}");
            if (removed > 0) changes.Add($"削除 {removed}");
            return changes.Count > 0
                ? $"{Items.Count} 件（未保存：{string.Join(" / ", changes)}）"
                : $"{Items.Count} 件";
        }
    }

    /// <summary>追加：選ばれた種類で1行追加する。名前は「種類名 + 連番」</summary>
    [RelayCommand]
    private void Add(ItemType type)
    {
        var number = Items.Count(i => i.Type == type) + 1;
        var row = CreateRow(new ItemData
        {
            TypeName = type.Name,
            Category = type.Category,
            Name = $"{type.Name} {number}",
        }, type);
        row.SetSaved(null);   // 保存したことがない行
        Items.Add(row);
        SelectedItem = row;
        Write("Add");
    }

    /// <summary>削除：その行を消す（行の「削除」ボタンから、行そのものが渡される）</summary>
    [RelayCommand]
    private void Remove(ItemRowViewModel row)
    {
        if (Items.Remove(row)) Write("Remove");
    }

    /// <summary>元に戻す / やり直し（設定ペインのものと同じ。設定と一覧の変更をまとめて1つの履歴で戻す）</summary>
    [RelayCommand(CanExecute = nameof(CanUndo))]
    private void Undo() => _settings.Undo();
    private bool CanUndo() => _settings.CanUndo;

    [RelayCommand(CanExecute = nameof(CanRedo))]
    private void Redo() => _settings.Redo();
    private bool CanRedo() => _settings.CanRedo;

    /// <summary>設定（Current.Items）から表を作り直す。選んでいた行は、残っていれば選び直す</summary>
    private void Load()
    {
        // 行は値を入れてから Edited をつなぐので、ここで Write は呼ばれない
        var selectedId = SelectedItem?.Id;
        Items.Clear();
        foreach (var data in _settings.Current.Items)
            Items.Add(CreateRow(data, FindType(data)));
        RefreshSaved();
        SelectedItem = Items.FirstOrDefault(i => i.Id == selectedId);
    }

    /// <summary>各行に「保存した時の値」を渡し直す（「新」「●」の印と件数の表示が更新される）</summary>
    private void RefreshSaved()
    {
        var saved = _settings.GetSaved().Items.DistinctBy(i => i.Id).ToDictionary(i => i.Id);
        _savedIds = saved.Keys.ToHashSet();
        foreach (var row in Items)
            row.SetSaved(saved.GetValueOrDefault(row.Id));
        OnPropertyChanged(nameof(Summary));
    }

    /// <summary>
    /// 表の全行を設定に書き写し、「未保存の変更」を更新する。書き写す前の状態を Undo 用に記録する。
    /// 同じ行の同じ項目を続けて変えた場合（文字入力など）は、1回分の Undo にまとまる。
    /// </summary>
    private void Write(string key)
    {
        _settings.Edit("Items." + key, s => s.Items = Items.Select(i => i.ToData()).ToList());
        OnPropertyChanged(nameof(Summary));
    }

    private ItemRowViewModel CreateRow(ItemData data, ItemType type)
    {
        var row = new ItemRowViewModel(type, data);
        row.Edited += (_, key) => Write($"{row.Id}.{key}");
        return row;
    }

    /// <summary>保存されていた種類名から ItemType を探す（一覧から消した種類なら、保存されていた名前のまま作る）</summary>
    private static ItemType FindType(ItemData data) =>
        AllItemTypes.FirstOrDefault(t => t.Name == data.TypeName) ?? new ItemType(data.TypeName, data.Category);

    private void OnItemsChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        foreach (ItemRowViewModel row in e.NewItems ?? Array.Empty<object>())
            row.PropertyChanged += OnRowPropertyChanged;
        foreach (ItemRowViewModel row in e.OldItems ?? Array.Empty<object>())
            row.PropertyChanged -= OnRowPropertyChanged;
        OnPropertyChanged(nameof(Summary));
    }

    private void OnRowPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(ItemRowViewModel.Status))
            OnPropertyChanged(nameof(Summary));
    }

    private static readonly ItemType[] AllItemTypes =
    {
        new("温度センサ", "センサ"),   new("湿度センサ", "センサ"),   new("圧力センサ", "センサ"),
        new("照度センサ", "センサ"),   new("加速度センサ", "センサ"), new("流量センサ", "センサ"),
        new("モーター", "アクチュエータ"), new("ソレノイド", "アクチュエータ"), new("ヒーター", "アクチュエータ"),
        new("ファン", "アクチュエータ"),   new("バルブ", "アクチュエータ"),
        new("カメラ", "入力"),         new("バーコードリーダ", "入力"), new("スイッチ", "入力"),
        new("表示灯", "出力"),         new("ブザー", "出力"),         new("ディスプレイ", "出力"),
        new("PLC", "制御"),            new("タイマー", "制御"),       new("カウンタ", "制御"),
        new("シリアル通信", "通信"),   new("TCP 接続", "通信"),       new("MQTT", "通信"),
        new("ログ出力", "その他"),     new("メモ", "その他"),
    };
}
