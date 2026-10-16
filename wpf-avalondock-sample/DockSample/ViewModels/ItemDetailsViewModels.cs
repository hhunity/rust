using CommunityToolkit.Mvvm.ComponentModel;

namespace DockSample.ViewModels;

/// <summary>
/// 一覧の1行の「詳細設定」の基底。種類の分類ごとに中身（項目）が違う。
/// 右側の詳細パネルには、この型ごとの DataTemplate（ItemListView.xaml）で表示される。
///
/// 「編集したか」は、今の値のスナップショットを、追加時（AcceptChanges 時）のものと比べて判定する。
/// 派生クラスは Snapshot() で自分の項目をまとめて返すだけでよい（タプルは値で比較される）。
/// </summary>
public abstract class ItemDetailsBase : ObservableObject
{
    private object? _original;

    /// <summary>詳細パネルの見出し（「センサの設定」など）</summary>
    public abstract string Title { get; }

    /// <summary>追加時から値が変わっているか</summary>
    public bool IsModified => !Equals(Snapshot(), _original);

    /// <summary>今の値を「元の値」にする</summary>
    public void AcceptChanges()
    {
        _original = Snapshot();
        OnPropertyChanged(nameof(IsModified));
    }

    /// <summary>派生クラスの項目をまとめた値（例：(A, B, C) のタプル）</summary>
    protected abstract object Snapshot();

    /// <summary>派生クラスのコンストラクタの最後に呼び、初期値を「元の値」にする</summary>
    protected void Initialize() => _original = Snapshot();

    /// <summary>項目が変わったら IsModified も通知する</summary>
    protected override void OnPropertyChanged(System.ComponentModel.PropertyChangedEventArgs e)
    {
        base.OnPropertyChanged(e);
        if (e.PropertyName != nameof(IsModified))
            base.OnPropertyChanged(new System.ComponentModel.PropertyChangedEventArgs(nameof(IsModified)));
    }

    /// <summary>種類の分類から、対応する詳細設定を作る</summary>
    public static ItemDetailsBase Create(ItemType type) => type.Category switch
    {
        "センサ"         => new SensorDetails(),
        "アクチュエータ" => new ActuatorDetails(),
        "通信"           => new CommunicationDetails(),
        _                => new GeneralDetails(),
    };
}

/// <summary>センサの設定</summary>
public partial class SensorDetails : ItemDetailsBase
{
    public SensorDetails() => Initialize();

    public override string Title => "センサの設定";

    /// <summary>単位の候補（コンボボックスに出す。直接入力も可）</summary>
    public IReadOnlyList<string> Units { get; } = new[] { "℃", "%", "kPa", "lx", "m/s²", "L/min" };

    [ObservableProperty] private int _channel = 1;
    [ObservableProperty] private int _samplingIntervalMs = 1000;
    [ObservableProperty] private string _unit = "℃";
    [ObservableProperty] private double _lowerLimit;
    [ObservableProperty] private double _upperLimit = 100;

    protected override object Snapshot() => (Channel, SamplingIntervalMs, Unit, LowerLimit, UpperLimit);
}

/// <summary>アクチュエータの設定</summary>
public partial class ActuatorDetails : ItemDetailsBase
{
    public ActuatorDetails() => Initialize();

    public override string Title => "アクチュエータの設定";

    [ObservableProperty] private int _channel = 1;
    [ObservableProperty] private double _maxOutputPercent = 100;
    [ObservableProperty] private bool _onAtStartup;

    protected override object Snapshot() => (Channel, MaxOutputPercent, OnAtStartup);
}

/// <summary>通信の設定</summary>
public partial class CommunicationDetails : ItemDetailsBase
{
    public CommunicationDetails() => Initialize();

    public override string Title => "通信の設定";

    [ObservableProperty] private string _host = "localhost";
    [ObservableProperty] private int _port = 1883;
    [ObservableProperty] private int _timeoutMs = 3000;
    [ObservableProperty] private bool _useTls;

    protected override object Snapshot() => (Host, Port, TimeoutMs, UseTls);
}

/// <summary>その他（入力・出力・制御など）の設定</summary>
public partial class GeneralDetails : ItemDetailsBase
{
    public GeneralDetails() => Initialize();

    public override string Title => "設定";

    [ObservableProperty] private string _address = "";
    [ObservableProperty] private int _channel = 1;

    protected override object Snapshot() => (Address, Channel);
}
