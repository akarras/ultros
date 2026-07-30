import json
import os

locale_files = {
    "en": "ultros-frontend/ultros-app/locales/en.json",
    "fr": "ultros-frontend/ultros-app/locales/fr.json",
    "de": "ultros-frontend/ultros-app/locales/de.json",
    "ja": "ultros-frontend/ultros-app/locales/ja.json",
    "cn": "ultros-frontend/ultros-app/locales/cn.json",
    "ko": "ultros-frontend/ultros-app/locales/ko.json",
    "tc": "ultros-frontend/ultros-app/locales/tc.json"
}

new_keys_by_locale = {
    "en": {
        "analyzer_tool_summary": "Find arbitrage deals by comparing cheapest listings on other worlds in your region against sales on your selected world.",
        "analyzer_tool_context": "Only lists items with observed sales within the 6-sale tracking buffer. Outlier filtering and tax inclusion can be adjusted in the Columns picker.",
        "analyzer_tool_help": "Flip Finder uses current lowest prices in your region and compares them to your target world's median sale price. Filter by velocity to make sure the item actually sells.",
        "analyzer_tooltip_velocity": "estimated sales/day",
        "analyzer_tooltip_drift": "% price change across recent sales",
        "analyzer_tooltip_confidence": "sample-size band from 30d stats",
        "analyzer_tooltip_profit_per_day": "profit ÷ expected days to sell",
        "analyzer_tooltip_trend": "7-day price trend",
        "analyzer_tooltip_sales_per_day": "30-day sales velocity (sales per day)",
        "analyzer_tooltip_volume_30d": "30-day total sales volume (number of sales)",
        "analyzer_columns_picker_desktop_only": "desktop only",
        "analyzer_last_sold_days_ago": "{{count}}d ago",
        "analyzer_last_sold_hours_ago": "{{count}}h ago",
        "analyzer_last_sold_just_now": "just now"
    },
    "fr": {
        "analyzer_tool_summary": "Trouvez des opportunités d'arbitrage en comparant les offres les moins chères sur d'autres mondes de votre région avec les ventes sur votre monde sélectionné.",
        "analyzer_tool_context": "Affiche uniquement les objets avec des ventes observées dans le tampon de suivi des 6 ventes. Le filtrage des valeurs aberrantes et l'inclusion des taxes peuvent être ajustés dans le sélecteur de colonnes.",
        "analyzer_tool_help": "Flip Finder utilise les prix les plus bas actuels dans votre région et les compare au prix de vente médian de votre monde cible. Filtrez par vélocité pour vous assurer que l'objet se vend réellement.",
        "analyzer_tooltip_velocity": "ventes estimées/jour",
        "analyzer_tooltip_drift": "% de variation de prix sur les ventes récentes",
        "analyzer_tooltip_confidence": "bande de taille d'échantillon des statistiques de 30 jours",
        "analyzer_tooltip_profit_per_day": "bénéfice ÷ jours de vente attendus",
        "analyzer_tooltip_trend": "tendance des prix sur 7 jours",
        "analyzer_tooltip_sales_per_day": "vitesse de vente sur 30 jours (ventes par jour)",
        "analyzer_tooltip_volume_30d": "volume total des ventes sur 30 jours (nombre de ventes)",
        "analyzer_columns_picker_desktop_only": "sur PC uniquement",
        "analyzer_last_sold_days_ago": "il y a {{count}}j",
        "analyzer_last_sold_hours_ago": "il y a {{count}}h",
        "analyzer_last_sold_just_now": "à l'instant"
    },
    "de": {
        "analyzer_tool_summary": "Finden Sie Arbitrage-Angebote, indem Sie die günstigsten Angebote auf anderen Welten in Ihrer Region mit den Verkäufen auf Ihrer ausgewählten Welt vergleichen.",
        "analyzer_tool_context": "Listet nur Gegenstände mit beobachteten Verkäufen innerhalb des 6-Verkauf-Tracking-Puffers auf. Ausreißerfilterung und Steuerberücksichtigung können im Spalten-Picker angepasst werden.",
        "analyzer_tool_help": "Flip Finder nutzt die aktuell niedrigsten Preise in Ihrer Region und vergleicht sie mit dem medianen Verkaufspreis Ihrer Zielwelt. Filtern Sie nach Verkaufsgeschwindigkeit, um sicherzustellen, dass sich der Gegenstand auch verkauft.",
        "analyzer_tooltip_velocity": "geschätzte Verkäufe/Tag",
        "analyzer_tooltip_drift": "% Preisänderung bei den jüngsten Verkäufen",
        "analyzer_tooltip_confidence": "Stichprobengrößenband aus 30-Tage-Statistiken",
        "analyzer_tooltip_profit_per_day": "Gewinn ÷ erwartete Verkaufstage",
        "analyzer_tooltip_trend": "7-Tage-Preistrend",
        "analyzer_tooltip_sales_per_day": "30-Tage-Verkaufsgeschwindigkeit (Verkäufe pro Tag)",
        "analyzer_tooltip_volume_30d": "30-Tage-Gesamtverkaufsvolumen (Anzahl der Verkäufe)",
        "analyzer_columns_picker_desktop_only": "nur Desktop",
        "analyzer_last_sold_days_ago": "vor {{count}} T.",
        "analyzer_last_sold_hours_ago": "vor {{count}} Std.",
        "analyzer_last_sold_just_now": "gerade eben"
    },
    "ja": {
        "analyzer_tool_summary": "選択したワールドでの販売実績と、同じデータセンター内の他ワールドの最安出品を比較して、アービトラージの機会を見つけます。",
        "analyzer_tool_context": "直近6件の販売履歴バッファ内で販売が確認されたアイテムのみを表示します。外れ値フィルタリングと税金込みの設定は「列選択」で調整できます。",
        "analyzer_tool_help": "フリップファインダーは、対象地域内の現在の最安価格を取得し、ターゲットワールドの販売中央値と比較します。アイテムが実際に売れるかどうか、販売速度でフィルタリングして確認してください。",
        "analyzer_tooltip_velocity": "推定販売数/日",
        "analyzer_tooltip_drift": "最近の販売における価格変動率(%)",
        "analyzer_tooltip_confidence": "30日間の統計に基づくサンプルサイズ帯",
        "analyzer_tooltip_profit_per_day": "利益 ÷ 推定販売日数",
        "analyzer_tooltip_trend": "7日間の価格推移",
        "analyzer_tooltip_sales_per_day": "30日間の販売速度（1日あたりの販売数）",
        "analyzer_tooltip_volume_30d": "30日間の合計販売量（販売回数）",
        "analyzer_columns_picker_desktop_only": "PCのみ",
        "analyzer_last_sold_days_ago": "{{count}}日前",
        "analyzer_last_sold_hours_ago": "{{count}}時間前",
        "analyzer_last_sold_just_now": "たった今"
    },
    "cn": {
        "analyzer_tool_summary": "通过将您所在区域其他服务器的最便宜上架商品与您所选服务器的销售情况进行比较，寻找套利交易的机会。",
        "analyzer_tool_context": "仅列出在 6 次销售跟踪缓存区内有观察到销售记录的物品。离群值过滤和包含税费可在列选择器中进行调整。",
        "analyzer_tool_help": "Flip Finder 使用您所在区域当前的最低价格，并将其与目标服务器的销售中位数价格进行比较。通过销售速度进行过滤，以确保该物品确实能够售出。",
        "analyzer_tooltip_velocity": "估计每日销量",
        "analyzer_tooltip_drift": "近期销售的价格变动百分比",
        "analyzer_tooltip_confidence": "基于 30 天统计数据的样本量区间",
        "analyzer_tooltip_profit_per_day": "利润 ÷ 预期售出天数",
        "analyzer_tooltip_trend": "7 天价格趋势",
        "analyzer_tooltip_sales_per_day": "30 天销售速度（每日销量）",
        "analyzer_tooltip_volume_30d": "30 天总销售量（销售次数）",
        "analyzer_columns_picker_desktop_only": "仅限桌面端",
        "analyzer_last_sold_days_ago": "{{count}}天前",
        "analyzer_last_sold_hours_ago": "{{count}}小时前",
        "analyzer_last_sold_just_now": "刚刚"
    },
    "ko": {
        "analyzer_tool_summary": "선택한 월드의 판매량과 해당 리전 내 다른 월드의 최저가 매물을 비교하여 차익 거래 기회를 찾습니다.",
        "analyzer_tool_context": "최근 6회 판매 기록 버퍼 내에 판매가 감지된 아이템만 나열합니다. 이상치 필터링 및 세금 포함 여부는 열 선택기에서 조정할 수 있습니다.",
        "analyzer_tool_help": "Flip Finder는 리전 내 현재 최저가를 타겟 월드의 판매 중간값과 비교합니다. 아이템이 실제로 판매되는지 확인하려면 판매 속도 필터를 사용하세요.",
        "analyzer_tooltip_velocity": "일일 예상 판매량",
        "analyzer_tooltip_drift": "최근 판매 가격 변동률(%)",
        "analyzer_tooltip_confidence": "30일 통계 기준 표본 크기 구간",
        "analyzer_tooltip_profit_per_day": "이익 ÷ 예상 판매 소요 일수",
        "analyzer_tooltip_trend": "7일 가격 추세",
        "analyzer_tooltip_sales_per_day": "30일 기준 판매 속도 (일일 판매량)",
        "analyzer_tooltip_volume_30d": "30일간 총 판매량 (판매 횟수)",
        "analyzer_columns_picker_desktop_only": "데스크톱 전용",
        "analyzer_last_sold_days_ago": "{{count}}일 전",
        "analyzer_last_sold_hours_ago": "{{count}}시간 전",
        "analyzer_last_sold_just_now": "방금 전"
    },
    "tc": {
        "analyzer_tool_summary": "通过將您所在區域其他伺服器的最便宜上架商品與您所選伺服器的銷售情況進行比較，尋找套利交易的機會。",
        "analyzer_tool_context": "僅列出在 6 次銷售跟蹤快取區內有觀察到銷售記錄的物品。離群值過濾和包含稅費可在列選擇器中進行調整。",
        "analyzer_tool_help": "Flip Finder 使用您所在區域當前的最低價格，並將其與目標伺服器的銷售中位數價格進行比較。通過銷售速度進行過濾，以確保該物品確實能夠售出。",
        "analyzer_tooltip_velocity": "估計每日銷量",
        "analyzer_tooltip_drift": "近期銷售 Price 變動百分比",
        "analyzer_tooltip_confidence": "基於 30 天統計數據的樣本量區間",
        "analyzer_tooltip_profit_per_day": "利潤 ÷ 預期售出天數",
        "analyzer_tooltip_trend": "7 天價格趨勢",
        "analyzer_tooltip_sales_per_day": "30 天銷售速度（每日銷量）",
        "analyzer_tooltip_volume_30d": "30 天總銷售量（銷售次數）",
        "analyzer_columns_picker_desktop_only": "僅限桌面端",
        "analyzer_last_sold_days_ago": "{{count}}天前",
        "analyzer_last_sold_hours_ago": "{{count}}小時前",
        "analyzer_last_sold_just_now": "剛剛"
    }
}

for lang, filepath in locale_files.items():
    print(f"Processing {lang} locale...")
    with open(filepath, "r", encoding="utf-8") as f:
        data = json.load(f)

    # 1. Add new keys
    for k, v in new_keys_by_locale[lang].items():
        data[k] = v

    # 2. Update existing keys if they exist and still say "The analyzer"
    # For en:
    # "analyzer_meta_desc": "The analyzer enables FFXIV merchants..." -> "Flip Finder enables..."
    # "analyzer_index_desc_1": "The analyzer helps find items..." -> "Flip Finder helps..."
    if lang == "en":
        data["analyzer_meta_desc"] = data["analyzer_meta_desc"].replace("The analyzer", "Flip Finder")
        data["analyzer_index_desc_1"] = data["analyzer_index_desc_1"].replace("The analyzer", "Flip Finder")
    elif lang == "fr":
        # French: "L'analyseur..." -> "Flip Finder..."
        data["analyzer_meta_desc"] = data["analyzer_meta_desc"].replace("L'analyseur", "Flip Finder")
        data["analyzer_index_desc_1"] = data["analyzer_index_desc_1"].replace("L'analyseur", "Flip Finder")
    elif lang == "de":
        # German: "Der Analyzer..." -> "Flip Finder..."
        data["analyzer_meta_desc"] = data["analyzer_meta_desc"].replace("Der Analyzer", "Flip Finder")
        data["analyzer_index_desc_1"] = data["analyzer_index_desc_1"].replace("Der Analyzer", "Flip Finder")
    elif lang == "ja":
        # Japanese: "分析器は..." -> "フリップファインダーは..."
        data["analyzer_meta_desc"] = data["analyzer_meta_desc"].replace("分析器", "フリップファインダー")
        data["analyzer_index_desc_1"] = data["analyzer_index_desc_1"].replace("分析器", "フリップファインダー")
    elif lang == "cn":
        # Chinese: "该分析器..." -> "Flip Finder..." or "分析器" -> "Flip Finder"
        data["analyzer_meta_desc"] = data["analyzer_meta_desc"].replace("分析器", "Flip Finder")
        data["analyzer_index_desc_1"] = data["analyzer_index_desc_1"].replace("分析器", "Flip Finder")
    elif lang == "ko":
        # Korean: "분석기는..." -> "Flip Finder는..." or similar
        data["analyzer_meta_desc"] = data["analyzer_meta_desc"].replace("분석기", "Flip Finder")
        data["analyzer_index_desc_1"] = data["analyzer_index_desc_1"].replace("분석기", "Flip Finder")
    elif lang == "tc":
        # Traditional Chinese: "分析器" -> "Flip Finder"
        data["analyzer_meta_desc"] = data["analyzer_meta_desc"].replace("分析器", "Flip Finder")
        data["analyzer_index_desc_1"] = data["analyzer_index_desc_1"].replace("分析器", "Flip Finder")

    with open(filepath, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=4, ensure_ascii=False)
        f.write("\n") # ensure a trailing newline

print("All locale files updated successfully.")
