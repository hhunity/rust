using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace DockSample.ViewModels;

/// <summary>
/// 追加できる項目の種類（「追加 ▾」の一覧に並ぶもの）。
/// ToString は DropDownButton の検索に使われるので、名前と分類を返す。
/// </summary>
public record ItemType(string Name, string Category)
{
    public override string ToString() => $"{Name} {Category}";
}

/// <summary>一覧の1行</summary>
public partial class ItemRowViewModel : ObservableObject
{
    // 追加したとき（または最後に AcceptChanges したとき）の値。これと比べて「編集した」かを判定する
    private string _originalName;
    private string _originalNotes;
    private bool _originalEnabled;

    public ItemRowViewModel(ItemType type, string name)
    {
        Type = type;
        _name = _originalName = name;
        _notes = _originalNotes = "";
        _enabled = _originalEnabled = true;
    }

    /// <summary>種類（追加時に決まり、変えられない）</summary>
    public ItemType Type { get; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsModified))]
    private string _name;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsModified))]
    private string _notes;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsModified))]
    private bool _enabled;

    /// <summary>追加したときから値が変わっているか（元の値に戻せば false に戻る）</summary>
    public bool IsModified =>
        Name != _originalName || Notes != _originalNotes || Enabled != _originalEnabled;

    /// <summary>今の値を「元の値」にする（保存したときなどに呼ぶ想定）</summary>
    public void AcceptChanges()
    {
        _originalName = Name;
        _originalNotes = Notes;
        _originalEnabled = Enabled;
        OnPropertyChanged(nameof(IsModified));
    }
}

/// <summary>
/// 「一覧」ペイン。上の「追加 ▾」（DropDownButton）で種類を選ぶと、表に1行追加される。
/// 各行：名前・種類・備考・有効・編集の印・削除ボタン
/// </summary>
public partial class ItemListViewModel : PaneViewModel
{
    public ItemListViewModel()
        : base("Document_Items", "項目の一覧", DockLocation.Document, buttonText: "一覧")
    {
        // 行の追加・削除や、各行の編集状態が変わったら、件数の表示を更新する
        Items.CollectionChanged += OnItemsChanged;
    }

    /// <summary>「追加 ▾」の一覧（数が多いので DropDownButton のスクロール・検索が効く）</summary>
    public IReadOnlyList<ItemType> ItemTypes => AllItemTypes;

    /// <summary>表の行</summary>
    public ObservableCollection<ItemRowViewModel> Items { get; } = new();

    /// <summary>表で選ばれている行（追加した行を選んだ状態にするのに使う）</summary>
    [ObservableProperty] private ItemRowViewModel? _selectedItem;

    /// <summary>表の上に出す「◯件（編集済み ◯件）」</summary>
    public string Summary
    {
        get
        {
            var modified = Items.Count(i => i.IsModified);
            return modified > 0 ? $"{Items.Count} 件（編集済み {modified} 件）" : $"{Items.Count} 件";
        }
    }

    /// <summary>追加：選ばれた種類で1行追加する。名前は「種類名 + 連番」</summary>
    [RelayCommand]
    private void Add(ItemType type)
    {
        var number = Items.Count(i => i.Type == type) + 1;
        var row = new ItemRowViewModel(type, $"{type.Name} {number}");
        Items.Add(row);
        SelectedItem = row;
    }

    /// <summary>削除：その行を消す（行の「削除」ボタンから、行そのものが渡される）</summary>
    [RelayCommand]
    private void Remove(ItemRowViewModel row) => Items.Remove(row);

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
        if (e.PropertyName == nameof(ItemRowViewModel.IsModified))
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
