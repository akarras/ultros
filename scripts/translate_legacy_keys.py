"""Translate keys whose values are still English in non-English locale files.

Only overwrites a locale value if it currently equals the English value (i.e. the
key was never translated). Run from repo root: `python scripts/translate_legacy_keys.py`.
"""
from __future__ import annotations

import json
from pathlib import Path

LOCALES_DIR = Path("ultros-frontend/ultros-app/locales")
LOCALES = ["de", "fr", "ja", "cn", "ko", "tc"]

# Each entry: key -> {locale: translation}
TRANSLATIONS: dict[str, dict[str, str]] = {
    # ---- nav / shell ----
    "side_nav_tools": {"de": "Werkzeuge", "fr": "Outils", "ja": "ツール", "cn": "工具", "ko": "도구", "tc": "工具"},
    "side_nav_saved": {"de": "Gespeichert", "fr": "Enregistré", "ja": "保存済み", "cn": "已保存", "ko": "저장됨", "tc": "已儲存"},
    "side_nav_toggle_sidebar": {"de": "Seitenleiste umschalten", "fr": "Basculer la barre latérale", "ja": "サイドバーを切替", "cn": "切换侧边栏", "ko": "사이드바 전환", "tc": "切換側邊欄"},
    "side_nav_toggle_navigation": {"de": "Navigation umschalten", "fr": "Basculer la navigation", "ja": "ナビゲーションを切替", "cn": "切换导航", "ko": "내비게이션 전환", "tc": "切換導覽"},
    "discord_bot": {"ja": "Discordボット"},
    "final_fantasy_copyright": {
        "ja": "FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. All Rights Reserved.",
        "cn": "FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. 保留所有权利。",
        "ko": "FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. All Rights Reserved.",
        "tc": "FINAL FANTASY XIV © 2010 - 2020 SQUARE ENIX CO., LTD. 保留所有權利。",
    },

    # ---- generic short labels ----
    "actions": {"fr": "Actions"},  # same in fr, keep
    "options": {"fr": "Options"},  # same in fr, keep
    "alchemist": {"de": "Alchemist"},  # FFXIV class name, German uses "Alchemist" too — keep
    "median_label": {"de": "Median"},  # German also uses "Median" — keep
    "status_label": {"de": "Status"},  # same in de — keep
    "theme_mode_system": {"de": "System"},  # de uses "System" too — keep
    "search_category_system": {"de": "System"},  # same — keep
    "theme_mode_label": {"fr": "Mode"},  # fr uses "Mode" too — keep
    "theme_palette_label": {"de": "Palette", "fr": "Palette"},  # same in both — keep
    "search_result_type_tool": {"de": "Werkzeug"},
    "search_result_type_page": {"fr": "Page"},  # same — keep

    # ---- "Item" → translate where appropriate ----
    "item": {"de": "Gegenstand"},
    "scrip_sources_item": {"de": "Gegenstand"},
    "create_alert_item_label": {"de": "Gegenstand"},
    "analyzer_col_item": {"de": "Gegenstand"},
    "vendor_resale_item": {"de": "Gegenstand"},
    "currency_exchange_table_item": {"de": "Gegenstand"},
    "retainers_item": {"de": "Gegenstand"},
    "list_view_item": {"de": "Gegenstand"},
    "alerts_col_item": {"de": "Gegenstand"},
    "leve_analyzer_col_leve_item": {"de": "Auftrag / Gegenstand"},
    "leve_item": {"de": "Auftrag / Gegenstand"},
    "item_explorer_items": {"de": "Gegenstände"},
    "item_explorer_name": {"de": "Name"},  # German also uses "Name" — keep
    "alerts_col_status": {"de": "Status"},  # keep

    # ---- iLvl / Lv stay as FFXIV abbreviations ----
    "item_explorer_ilvl": {"de": "iGS", "fr": "iLvl"},  # German FFXIV uses "iGS" (item-Gegenstandsstufe)
    "item_explorer_ilvl_prefix": {"de": "iGS", "fr": "iLvl"},
    "related_ilvl_prefix": {"de": "iGS", "ko": "iLvl", "tc": "iLvl"},
    "sale_history_stat_hq_percent": {"de": "HQ %", "ko": "HQ %", "tc": "HQ %"},  # keep
    "cheapest_hq_prefix": {"de": "HQ: ", "ja": "HQ: ", "ko": "HQ: "},  # keep
    "stats_hq_prefix": {"de": "(HQ: ", "ko": "(HQ: "},  # keep
    "related_recipe_hq_label": {"de": "HQ:", "ja": "HQ:", "ko": "HQ:"},  # keep
    "related_recipe_lq_label": {"de": "NQ:", "ja": "NQ:", "ko": "NQ:"},  # use NQ which is FFXIV's "non-HQ" everywhere

    # ---- ROI / Profit (Min) — these are filter labels; leave as same-language idioms ----
    # ROI is widely used as-is across most locales; we'll leave the abbreviations.
    # The full-text labels we leave as in en.
    "analyzer_filter_profit_min_label": {"de": "Profit (Min)", "fr": "Profit (Min)"},  # same — keep
    "analyzer_filter_roi_min_label": {"de": "ROI (Min)", "fr": "ROI (Min)"},  # same — keep
    "recipe_analyzer_filter_profit_min_label": {"de": "Profit (Min)", "fr": "Profit (Min)"},
    "recipe_analyzer_filter_roi_min_label": {"de": "ROI (Min)", "fr": "ROI (Min)"},
    "leve_analyzer_filter_profit_min_label": {"de": "Profit (Min)", "fr": "Profit (Min)"},
    "venture_analyzer_filter_profit_min_label": {"de": "Profit (Min)", "fr": "Profit (Min)"},
    "vendor_resale_filter_profit_min_label": {"de": "Profit (Min)", "fr": "Profit (Min)"},
    "vendor_resale_filter_roi_min_label": {"de": "ROI (Min)", "fr": "ROI (Min)"},
    "fc_crafting_filter_profit_min_label": {"de": "Profit (Min)", "fr": "Profit (Min)"},
    "fc_crafting_filter_roi_min_label": {"de": "ROI (Min)", "fr": "ROI (Min)"},
    "analyzer_roi_gte": {"de": "ROI ≥ ", "fr": "ROI ≥ ", "ja": "ROI ≥ ", "cn": "ROI ≥ ", "ko": "ROI ≥ ", "tc": "ROI ≥ "},  # keep
    "roi_gte": {"de": "ROI ≥ ", "fr": "ROI ≥ ", "ja": "ROI ≥ ", "cn": "ROI ≥ ", "ko": "ROI ≥ ", "tc": "ROI ≥ "},
    "vendor_resale_roi_gte": {"de": "ROI ≥ ", "fr": "ROI ≥ ", "ja": "ROI ≥ ", "cn": "ROI ≥ ", "ko": "ROI ≥ ", "tc": "ROI ≥ "},
    "analyzer_budget_lte": {"de": "Budget ≤ ", "fr": "Budget ≤ "},
    "vendor_resale_arbitrage": {"de": "Arbitrage", "fr": "Arbitrage"},  # same word — keep
    "leve_analyzer_options": {"fr": "Options"},  # keep
    "venture_analyzer_options": {"fr": "Options"},  # keep
    "list_view_options": {"fr": "Options"},  # keep
    "retainers_total": {"fr": "Total"},  # same — keep

    # ---- chart strings ----
    "chart_range_24h": {"de": "24h", "fr": "24h"},  # keep
    "chart_range_90d": {"ja": "90日", "tc": "90日"},
    "chart_color_by": {"de": "Färben nach:", "fr": "Couleur selon :", "ja": "色分け:", "cn": "颜色按：", "ko": "색상 기준:", "tc": "顏色依："},
    "chart_color_region": {"de": "Region", "fr": "Région", "ja": "リージョン", "cn": "大区", "ko": "리전", "tc": "大區"},
    "chart_color_datacenter": {"de": "Rechenzentrum", "fr": "Datacenter", "ja": "データセンター", "cn": "数据中心", "ko": "데이터센터", "tc": "資料中心"},
    "chart_color_world": {"de": "Welt", "fr": "Monde", "ja": "ワールド", "cn": "服务器", "ko": "월드", "tc": "伺服器"},
    "chart_legend_market_avg": {"de": "Marktdurchschnitt", "fr": "Moyenne du marché", "ja": "マーケット平均", "cn": "市场均价", "ko": "시장 평균", "tc": "市場均價"},
    "chart_legend_quantity": {"de": "Menge", "fr": "Quantité", "ja": "数量", "cn": "数量", "ko": "수량", "tc": "數量"},
    "chart_legend_trend": {"de": "Trend"},  # same — keep
    "chart_stat_market_avg": {"de": "Marktdurchschnitt", "fr": "moyenne du marché", "ja": "マーケット平均", "cn": "市场均价", "ko": "시장 평균", "tc": "市場均價"},
    "chart_stat_max": {"fr": "max"},  # same — keep
    "chart_stat_min": {"fr": "min"},  # same — keep
    "chart_toggle_market_avg": {"de": "Marktdurchschnitt", "fr": "Moyenne du marché", "ja": "マーケット平均", "cn": "市场均价", "ko": "시장 평균", "tc": "市場均價"},
    "sale_history_stat_max": {"de": "Max", "fr": "Max"},  # keep
    "sale_history_stat_min": {"de": "Min", "fr": "Min"},  # keep
    "sale_history_insights_subtitle": {"de": "Aktuelle Markt-Velocity", "fr": "Vélocité de marché récente", "ja": "直近のマーケット回転", "cn": "近期市场周转", "ko": "최근 시장 회전", "tc": "近期市場周轉"},

    # ---- currency exchange ----
    "currency_exchange_max": {"de": "Max", "fr": "Max"},  # keep
    "currency_exchange_min": {"de": "Min", "fr": "Min"},  # keep
    "currency_exchange_max_field_aria": {"de": "Max {{name}}", "fr": "Max {{name}}"},  # keep
    "currency_exchange_min_field_aria": {"de": "Min {{name}}", "fr": "Min {{name}}"},  # keep

    # ---- recipe analyzer ----
    "recipe_analyzer_item_level_label": {"de": "Lv {{level}} • iGS {{ilvl}}", "ja": "Lv {{level}} • iLv {{ilvl}}", "cn": "Lv {{level}} • iLv {{ilvl}}", "ko": "Lv {{level}} • iLv {{ilvl}}", "tc": "Lv {{level}} • iLv {{ilvl}}"},
    "recipe_analyzer_sub_suffix": {"ja": "サブ"},
    "recipe_analyzer_subcraft_row": {"fr": "• {{count}}× {{name}} ({{gil}} gil)\n"},
    "recipe_analyzer_tool_summary": {
        "de": "Finde Rezepte, bei denen die geschätzten Herstellungskosten unter dem Marktpreis des Endprodukts liegen.",
        "fr": "Trouvez les recettes dont le coût de craft estimé est inférieur au prix de marché du produit fini.",
        "ja": "推定製作コストが完成品のマーケット価格を下回るレシピを探します。",
        "cn": "查找估算制作成本低于成品市场价的配方。",
        "ko": "추정 제작 비용이 완성품의 시장가보다 낮은 레시피를 찾습니다.",
        "tc": "查找估算製作成本低於成品市場價的配方。",
    },
    "recipe_analyzer_tool_context": {
        "de": "Stelle zuerst die Handwerkerstufen ein, damit die Ergebnisse zu Rezepten passen, die du tatsächlich herstellen kannst.",
        "fr": "Configurez d'abord les niveaux d'artisan pour que les résultats correspondent aux recettes que vous pouvez réellement fabriquer.",
        "ja": "実際に作成できるレシピに結果を絞り込むため、まずクラフタークラスのレベルを設定してください。",
        "cn": "先设置生产职业等级，使结果只显示你实际能制作的配方。",
        "ko": "실제로 만들 수 있는 레시피만 표시되도록 먼저 제작 직업 레벨을 설정하세요.",
        "tc": "先設定生產職業等級，使結果只顯示你實際能製作的配方。",
    },
    "recipe_analyzer_tool_help": {
        "de": "Recipe Analyzer nutzt die günstigsten Zutaten-Angebote, optionale Sub-Craft-Prüfungen, deine Handwerkerstufen und kürzliche Verkäufe. Ein profitables Rezept ist am stärksten, wenn das Ergebnis auch regelmäßig verkauft wird.",
        "fr": "Recipe Analyzer utilise les annonces d'ingrédients les moins chères, les vérifications optionnelles de sous-craft, vos niveaux d'artisan et les ventes récentes. Une recette rentable est la plus solide quand le produit fini se vend aussi régulièrement.",
        "ja": "Recipe Analyzerは最安の素材出品・任意のサブクラフト判定・クラフターレベル・直近の販売実績を組み合わせます。完成品が定期的に売れているときに、利益の出るレシピは最も信頼できます。",
        "cn": "Recipe Analyzer 综合最低素材挂单、可选子制作判定、你的生产职业等级与最近成交。当产出物也能稳定卖出时，盈利配方最具说服力。",
        "ko": "Recipe Analyzer는 최저가 재료 매물, 선택적 하위 제작 검증, 본인의 제작 직업 레벨, 최근 판매를 결합합니다. 결과물도 꾸준히 판매될 때 수익 레시피의 신뢰도가 가장 높습니다.",
        "tc": "Recipe Analyzer 綜合最低素材掛單、可選子製作判定、你的生產職業等級與最近成交。當產出物也能穩定賣出時，盈利配方最具說服力。",
    },

    # ---- placeholders that stay literal ----
    "analyzer_placeholder_0_to_6": {"de": "0–6", "fr": "0–6", "ja": "0–6", "cn": "0–6", "ko": "0–6", "tc": "0–6"},  # keep
    "on_hand_placeholder_zero": {"de": "0", "fr": "0", "ja": "0", "cn": "0", "ko": "0", "tc": "0"},  # keep

    # ---- list sharing / subscriptions ----
    "shared_with_me": {"de": "Mit mir geteilt", "fr": "Partagé avec moi", "ja": "共有されたもの", "cn": "与我共享", "ko": "나와 공유됨", "tc": "與我共享"},
    "no_owned_lists_but_shared": {
        "de": "Du besitzt noch keine Listen — aber andere haben {{count}} mit dir geteilt.",
        "fr": "Vous ne possédez aucune liste — mais d'autres en ont partagé {{count}} avec vous.",
        "ja": "まだリストを所有していません — でも他のユーザーから {{count}} 件共有されています。",
        "cn": "你还没有自己的清单 — 但其他人已与你分享了 {{count}} 个。",
        "ko": "아직 소유한 리스트는 없지만 다른 사람이 {{count}}개를 공유했습니다.",
        "tc": "你還沒有自己的清單 — 但其他人已與你分享了 {{count}} 個。",
    },
    "leave_list": {"de": "Liste verlassen", "fr": "Quitter la liste", "ja": "リストから離脱", "cn": "退出清单", "ko": "리스트 나가기", "tc": "退出清單"},
    "leave_list_confirm": {"de": "Dieser geteilten Liste nicht mehr folgen?", "fr": "Cesser de suivre cette liste partagée ?", "ja": "この共有リストの購読を解除しますか？", "cn": "停止跟随这个共享清单？", "ko": "이 공유 리스트를 그만 따라가시겠습니까?", "tc": "停止跟隨這個共享清單？"},
    "leave_list_tooltip": {"de": "Dich von dieser geteilten Liste entfernen", "fr": "Vous retirer de cette liste partagée", "ja": "この共有リストから自分を外します", "cn": "将自己从此共享清单中移除", "ko": "이 공유 리스트에서 자신을 제거", "tc": "將自己從此共享清單中移除"},
    "add_to_list_read_only": {"de": "Nur Lesen — keine Items hinzufügbar", "fr": "Lecture seule — impossible d'ajouter", "ja": "読み取り専用 — アイテムを追加できません", "cn": "只读 — 无法添加物品", "ko": "읽기 전용 — 아이템을 추가할 수 없음", "tc": "唯讀 — 無法新增物品"},
    "list_shared_editor_badge": {"de": "Geteilt · Editor", "fr": "Partagé · Éditeur", "ja": "共有 · 編集者", "cn": "共享 · 编辑者", "ko": "공유 · 편집자", "tc": "共享 · 編輯者"},
    "list_shared_viewer_badge": {"de": "Geteilt · Betrachter", "fr": "Partagé · Lecteur", "ja": "共有 · 閲覧者", "cn": "共享 · 查看者", "ko": "공유 · 보기 전용", "tc": "共享 · 檢視者"},
    "lists_page_title": {"de": "Listen", "fr": "Listes", "ja": "リスト", "cn": "清单", "ko": "리스트", "tc": "清單"},

    "list_item_row_target_price_label": {"de": "Zielpreis (Gil)", "fr": "Prix cible (gil)", "ja": "目標価格 (ギル)", "cn": "目标价格 (gil)", "ko": "목표 가격 (길)", "tc": "目標價格 (gil)"},
    "list_item_row_unavailable_on_market": {"de": "Nicht auf dem Marktbrett verfügbar", "fr": "Indisponible sur le marché", "ja": "マーケットボードに出品なし", "cn": "市场板上不可用", "ko": "마켓보드에서 사용 불가", "tc": "市場板上不可用"},
    "list_subscribe_description": {
        "de": "Du wirst benachrichtigt, sobald ein Item dieser Liste auf oder unter den Zielpreis fällt. Setze pro Item Zielpreise auf der Listenseite.",
        "fr": "Vous serez notifié dès qu'un objet de cette liste descend au prix cible ou en dessous. Définissez les cibles par objet sur la page de la liste.",
        "ja": "リスト内のアイテムが目標価格以下になったら通知します。アイテムごとの目標はリストページで設定してください。",
        "cn": "当此清单中任意物品价格降至目标价或更低时通知你。可在清单页面为每个物品设置目标价。",
        "ko": "이 리스트의 아이템이 목표 가격 이하로 떨어지면 알려드립니다. 아이템별 목표가는 리스트 페이지에서 설정하세요.",
        "tc": "當此清單中任意物品價格降至目標價或更低時通知你。可在清單頁面為每個物品設定目標價。",
    },
    "list_subscribe_price_targets_mode": {"de": "Zielpreise", "fr": "Prix cibles", "ja": "目標価格", "cn": "目标价格", "ko": "목표 가격", "tc": "目標價格"},
    "list_subscribe_submit": {"de": "Abonnieren", "fr": "S'abonner", "ja": "購読する", "cn": "订阅", "ko": "구독", "tc": "訂閱"},
    "list_subscribe_success_toast": {"de": "Liste abonniert", "fr": "Liste abonnée", "ja": "リストを購読しました", "cn": "已订阅清单", "ko": "리스트를 구독했습니다", "tc": "已訂閱清單"},
    "list_subscribe_title": {"de": "Benachrichtige mich für diese Liste: {{name}}", "fr": "Me notifier pour cette liste : {{name}}", "ja": "このリストの通知を受け取る: {{name}}", "cn": "为此清单接收通知：{{name}}", "ko": "이 리스트 알림 받기: {{name}}", "tc": "為此清單接收通知：{{name}}"},
    "list_subscribe_updates_mode": {"de": "Listen-Updates", "fr": "Mises à jour de liste", "ja": "リストの更新", "cn": "清单更新", "ko": "리스트 업데이트", "tc": "清單更新"},
    "list_update_subscribe_description": {
        "de": "Du wirst benachrichtigt, wenn diese Liste oder eine ihrer Zeilen geändert wird.",
        "fr": "Vous serez notifié lorsque cette liste ou l'une de ses lignes change.",
        "ja": "このリストやその行が変更されたときに通知されます。",
        "cn": "当此清单或其中某一行发生变更时，会通知你。",
        "ko": "이 리스트나 항목이 변경되면 알림을 받습니다.",
        "tc": "當此清單或其中某一列發生變更時，會通知你。",
    },
    "list_update_subscribe_submit": {"de": "Updates abonnieren", "fr": "S'abonner aux mises à jour", "ja": "更新を購読する", "cn": "订阅更新", "ko": "업데이트 구독", "tc": "訂閱更新"},
    "list_update_subscribe_success_toast": {"de": "Listen-Updates abonniert", "fr": "Mises à jour de liste abonnées", "ja": "リスト更新を購読しました", "cn": "已订阅清单更新", "ko": "리스트 업데이트를 구독했습니다", "tc": "已訂閱清單更新"},
    "list_view_subscribe_aria": {"de": "Mich für diese Liste benachrichtigen", "fr": "Me notifier pour cette liste", "ja": "このリストの通知を受け取る", "cn": "为此清单接收通知", "ko": "이 리스트 알림 받기", "tc": "為此清單接收通知"},
    "list_view_subscribe_button": {"de": "Benachrichtigen", "fr": "Me notifier", "ja": "通知を受け取る", "cn": "接收通知", "ko": "알림 받기", "tc": "接收通知"},
    "list_view_subscribe_tooltip": {"de": "Preis-Alerts für jedes Item mit Zielpreis in dieser Liste abonnieren", "fr": "S'abonner aux alertes prix pour chaque objet ciblé de cette liste", "ja": "このリストの目標価格付きアイテムすべての価格通知を購読", "cn": "为此清单中每个设置了目标价的物品订阅价格提醒", "ko": "이 리스트의 가격이 지정된 모든 아이템 가격 알림 구독", "tc": "為此清單中每個設定了目標價的物品訂閱價格提醒"},

    "listings_col_total": {"fr": "total"},  # keep
    "sale_history_col_total": {"fr": "total"},  # keep
    "top_deals_profit_label": {"fr": "profit"},  # keep
    "trends_world_context_prefix": {"de": "Welt-Kontext: ", "fr": "Contexte du monde : ", "ja": "ワールド情報: ", "cn": "服务器情境：", "ko": "월드 컨텍스트: ", "tc": "伺服器情境："},

    # ---- alerts / endpoints ----
    "alerts_tab_endpoints": {"de": "Endpunkte", "fr": "Points de distribution", "ja": "配信先", "cn": "通知端点", "ko": "엔드포인트", "tc": "通知端點"},
    "alerts_tab_history": {"de": "Verlauf", "fr": "Historique", "ja": "履歴", "cn": "历史", "ko": "기록", "tc": "歷史"},
    "alerts_tab_rules": {"de": "Alarmregeln", "fr": "Règles d'alerte", "ja": "アラートルール", "cn": "提醒规则", "ko": "알림 규칙", "tc": "提醒規則"},
    "alerts_list_defined_world": {"de": "listendefiniert", "fr": "défini par la liste", "ja": "リスト定義", "cn": "清单定义", "ko": "리스트 지정", "tc": "清單定義"},
    "alerts_list_price_target": {"de": "Ziel pro Item", "fr": "cible par objet", "ja": "アイテムごとの目標", "cn": "按物品设置目标", "ko": "아이템별 목표", "tc": "按物品設定目標"},
    "alerts_list_update_rule": {"de": "Listen-Updates", "fr": "mises à jour de liste", "ja": "リストの更新", "cn": "清单更新", "ko": "리스트 업데이트", "tc": "清單更新"},
    "alerts_margin_percent": {"de": "{{margin}} % Marge", "fr": "{{margin}}% de marge", "ja": "{{margin}}% マージン", "cn": "{{margin}}% 利润率", "ko": "{{margin}}% 마진", "tc": "{{margin}}% 利潤率"},
    "alerts_retainer_undercut_rule": {"de": "Gehilfen-Unterbietungen", "fr": "Sous-cotations de serviteur", "ja": "雇員のアンダーカット", "cn": "雇员被低价压过", "ko": "모험가 가격 인하 감지", "tc": "雇員被低價壓過"},
    "alerts_delivery_discord_dm": {"ja": "Discord DM", "ko": "디스코드 DM"},
    "alerts_delivery_webhook": {"de": "Webhook", "fr": "Webhook", "ja": "Webhook", "cn": "Webhook", "tc": "Webhook"},  # technical term — keep
    "alert_drawer_discord_dm": {"ja": "Discord DM", "ko": "디스코드 DM"},
    "alert_drawer_webhook": {"de": "Webhook", "fr": "Webhook", "ja": "Webhook", "cn": "Webhook", "tc": "Webhook"},
    "alert_drawer_deliver_to": {"de": "Zustellen an", "fr": "Livrer à", "ja": "配信先", "cn": "发送到", "ko": "전송 대상", "tc": "發送至"},
    "alert_drawer_err_endpoint_required": {"de": "Wähle mindestens einen Endpunkt", "fr": "Choisissez au moins un point de distribution", "ja": "配信先を最低1つ選択してください", "cn": "请至少选择一个通知端点", "ko": "엔드포인트를 하나 이상 선택하세요", "tc": "請至少選擇一個通知端點"},
    "alert_drawer_loading_endpoints": {"de": "Endpunkte werden geladen...", "fr": "Chargement des points de distribution...", "ja": "配信先を読み込み中...", "cn": "正在加载通知端点...", "ko": "엔드포인트 불러오는 중...", "tc": "正在載入通知端點..."},
    "alert_drawer_no_endpoints_link": {"de": "Einen hinzufügen", "fr": "En ajouter un", "ja": "追加する", "cn": "添加一个", "ko": "추가하기", "tc": "新增一個"},
    "alert_drawer_no_endpoints_prefix": {"de": "Noch keine Endpunkte. ", "fr": "Aucun point de distribution. ", "ja": "配信先がまだありません。", "cn": "尚无通知端点。", "ko": "엔드포인트가 아직 없습니다. ", "tc": "尚無通知端點。"},
    "alert_drawer_no_endpoints_suffix": {"de": " bevor du Alarme erstellst.", "fr": " avant de créer des alertes.", "ja": " アラートを作成する前に。", "cn": " 后再创建提醒。", "ko": " 알림을 만들기 전에.", "tc": " 後再建立提醒。"},

    "endpoints_heading": {"de": "Endpunkte", "fr": "Points de distribution", "ja": "配信先", "cn": "通知端点", "ko": "엔드포인트", "tc": "通知端點"},
    "endpoints_add_endpoint": {"de": "Endpunkt hinzufügen", "fr": "Ajouter un point de distribution", "ja": "配信先を追加", "cn": "添加通知端点", "ko": "엔드포인트 추가", "tc": "新增通知端點"},
    "endpoints_browser_push_enabled_toast": {"de": "Browser-Benachrichtigungen aktiviert", "fr": "Notifications navigateur activées", "ja": "ブラウザ通知を有効化しました", "cn": "已启用浏览器通知", "ko": "브라우저 알림을 활성화했습니다", "tc": "已啟用瀏覽器通知"},
    "endpoints_channel_label": {"de": "Kanal", "fr": "Salon", "ja": "チャンネル", "cn": "频道", "ko": "채널", "tc": "頻道"},
    "endpoints_create_button": {"de": "Erstellen", "fr": "Créer", "ja": "作成", "cn": "创建", "ko": "생성", "tc": "建立"},
    "endpoints_created_toast": {"de": "Endpunkt erstellt", "fr": "Point de distribution créé", "ja": "配信先を作成しました", "cn": "已创建通知端点", "ko": "엔드포인트가 생성되었습니다", "tc": "已建立通知端點"},
    "endpoints_delete_aria": {"de": "Endpunkt löschen", "fr": "Supprimer le point de distribution", "ja": "配信先を削除", "cn": "删除通知端点", "ko": "엔드포인트 삭제", "tc": "刪除通知端點"},
    "endpoints_deleted_toast": {"de": "Endpunkt gelöscht", "fr": "Point de distribution supprimé", "ja": "配信先を削除しました", "cn": "已删除通知端点", "ko": "엔드포인트를 삭제했습니다", "tc": "已刪除通知端點"},
    "endpoints_delivery_failed": {"de": "Zustellung fehlgeschlagen", "fr": "Échec de livraison", "ja": "配信に失敗しました", "cn": "发送失败", "ko": "전송 실패", "tc": "發送失敗"},
    "endpoints_discord_channel": {"de": "Discord-Kanal", "fr": "Salon Discord", "ja": "Discordチャンネル", "cn": "Discord 频道", "ko": "디스코드 채널", "tc": "Discord 頻道"},
    "endpoints_discord_dm_me": {"de": "Discord-DM (an mich)", "fr": "DM Discord (à moi)", "ja": "Discord DM (自分宛)", "cn": "Discord 私信（给我）", "ko": "디스코드 DM (나에게)", "tc": "Discord 私訊（給我）"},
    "endpoints_empty_state": {"de": "Noch keine Endpunkte. Füge einen hinzu, um Alarme zu erhalten.", "fr": "Aucun point de distribution. Ajoutez-en un pour recevoir des alertes.", "ja": "配信先がまだありません。アラートを受け取るには追加してください。", "cn": "尚无通知端点。添加一个以接收提醒。", "ko": "엔드포인트가 없습니다. 알림을 받으려면 추가하세요.", "tc": "尚無通知端點。新增一個以接收提醒。"},
    "endpoints_enable_browser_push": {"de": "Browser-Benachrichtigungen aktivieren", "fr": "Activer les notifications du navigateur", "ja": "ブラウザ通知を有効化", "cn": "启用浏览器通知", "ko": "브라우저 알림 활성화", "tc": "啟用瀏覽器通知"},
    "endpoints_enable_browser_push_title": {"de": "Diesen Browser für Push-Benachrichtigungen abonnieren, wenn deine Alarme auslösen", "fr": "Abonner ce navigateur pour recevoir les notifications push lors du déclenchement des alertes", "ja": "アラート発火時にプッシュ通知を受け取るためこのブラウザを購読", "cn": "订阅此浏览器，当提醒触发时接收推送通知", "ko": "알림이 발생할 때 푸시 알림을 받도록 이 브라우저를 구독", "tc": "訂閱此瀏覽器，當提醒觸發時接收推送通知"},
    "endpoints_err_channel_required": {"de": "Wähle einen Discord-Kanal", "fr": "Choisissez un salon Discord", "ja": "Discordチャンネルを選択してください", "cn": "请选择一个 Discord 频道", "ko": "디스코드 채널을 선택하세요", "tc": "請選擇一個 Discord 頻道"},
    "endpoints_err_name_required": {"de": "Name ist erforderlich", "fr": "Le nom est requis", "ja": "名前は必須です", "cn": "名称必填", "ko": "이름은 필수입니다", "tc": "名稱必填"},
    "endpoints_loading_discord_servers": {"de": "Discord-Server werden geladen...", "fr": "Chargement des serveurs Discord...", "ja": "Discordサーバーを読み込み中...", "cn": "正在加载 Discord 服务器...", "ko": "디스코드 서버 불러오는 중...", "tc": "正在載入 Discord 伺服器..."},
    "endpoints_method_discord_channel": {"de": "Discord · #{{channel}}", "fr": "Discord · #{{channel}}", "ja": "Discord · #{{channel}}", "cn": "Discord · #{{channel}}", "ko": "디스코드 · #{{channel}}", "tc": "Discord · #{{channel}}"},
    "endpoints_method_discord_channel_id": {"de": "Discord-Kanal {{channel_id}}", "fr": "Salon Discord {{channel_id}}", "ja": "Discordチャンネル {{channel_id}}", "cn": "Discord 频道 {{channel_id}}", "ko": "디스코드 채널 {{channel_id}}", "tc": "Discord 頻道 {{channel_id}}"},
    "endpoints_method_discord_channel_in_guild": {"de": "Discord · #{{channel}} in {{guild}}", "fr": "Discord · #{{channel}} dans {{guild}}", "ja": "Discord · {{guild}} の #{{channel}}", "cn": "Discord · {{guild}} 的 #{{channel}}", "ko": "디스코드 · {{guild}} 의 #{{channel}}", "tc": "Discord · {{guild}} 的 #{{channel}}"},
    "endpoints_method_discord_dm": {"de": "Discord-DM", "fr": "DM Discord", "ja": "Discord DM", "cn": "Discord 私信", "ko": "디스코드 DM", "tc": "Discord 私訊"},
    "endpoints_method_label": {"de": "Methode", "fr": "Méthode", "ja": "方法", "cn": "方式", "ko": "방식", "tc": "方式"},
    "endpoints_method_web_push": {"de": "Browser-Push", "fr": "Push navigateur", "ja": "ブラウザプッシュ", "cn": "浏览器推送", "ko": "브라우저 푸시", "tc": "瀏覽器推送"},
    "endpoints_method_webhook": {"de": "Webhook", "fr": "Webhook", "ja": "Webhook", "cn": "Webhook", "ko": "Webhook", "tc": "Webhook"},
    "endpoints_name_label": {"de": "Name", "fr": "Nom", "ja": "名前", "cn": "名称", "ko": "이름", "tc": "名稱"},
    "endpoints_no_discord_servers": {"de": "Keine gemeinsamen Discord-Server, in denen du den Server verwalten kannst und der Bot in einen Kanal schreiben darf.", "fr": "Aucun serveur Discord partagé où vous pouvez gérer le serveur et où le bot peut écrire dans un salon.", "ja": "サーバー管理権限があり、かつボットがチャンネルに書き込める共通のDiscordサーバーがありません。", "cn": "没有你既能管理、Bot 又能在频道中写入的共同 Discord 服务器。", "ko": "관리 권한이 있으면서 봇이 채널에 쓸 수 있는 공통 디스코드 서버가 없습니다.", "tc": "沒有你既能管理、Bot 又能在頻道中寫入的共同 Discord 伺服器。"},
    "endpoints_test_button": {"de": "Testen", "fr": "Tester", "ja": "テスト", "cn": "测试", "ko": "테스트", "tc": "測試"},
    "endpoints_test_delivered_toast": {"de": "Test zugestellt", "fr": "Test livré", "ja": "テストを配信しました", "cn": "测试已送达", "ko": "테스트를 전송했습니다", "tc": "測試已送達"},
    "endpoints_webhook_url": {"de": "Webhook-URL", "fr": "URL du webhook", "ja": "Webhook URL", "cn": "Webhook URL", "ko": "Webhook URL", "tc": "Webhook URL"},

    # ---- undercut alert ----
    "undercut_alert_created_toast": {"de": "Unterbietungs-Alarm erstellt", "fr": "Alerte de sous-cotation créée", "ja": "アンダーカットアラートを作成しました", "cn": "已创建被低价压过提醒", "ko": "가격 인하 알림을 만들었습니다", "tc": "已建立被低價壓過提醒"},
    "undercut_alert_description": {"de": "Lass dich benachrichtigen, wenn ein anderes Angebot einen deiner beanspruchten Gehilfen unterbietet.", "fr": "Soyez notifié lorsqu'une autre annonce sous-cote l'un de vos serviteurs revendiqués.", "ja": "あなたの登録雇員の出品が他の出品にアンダーカットされたときに通知します。", "cn": "当其他挂单低于你认领的雇员价格时通知你。", "ko": "다른 매물이 등록된 모험가 가격보다 낮게 올라오면 알려드립니다.", "tc": "當其他掛單低於你認領的雇員價格時通知你。"},
    "undercut_alert_err_margin_number": {"de": "Marge muss eine Zahl sein", "fr": "La marge doit être un nombre", "ja": "マージンは数値である必要があります", "cn": "利润率必须为数字", "ko": "마진은 숫자여야 합니다", "tc": "利潤率必須為數字"},
    "undercut_alert_err_margin_range": {"de": "Marge muss zwischen 0 und 200 liegen", "fr": "La marge doit être comprise entre 0 et 200", "ja": "マージンは0から200の間にしてください", "cn": "利润率必须在 0 到 200 之间", "ko": "마진은 0에서 200 사이여야 합니다", "tc": "利潤率必須在 0 到 200 之間"},
    "undercut_alert_margin_label": {"de": "Margen-Prozentsatz", "fr": "Pourcentage de marge", "ja": "マージン (%)", "cn": "利润率百分比", "ko": "마진 퍼센트", "tc": "利潤率百分比"},
    "undercut_alert_open_button": {"de": "Unterbietungs-Alarm", "fr": "Alerte de sous-cotation", "ja": "アンダーカットアラート", "cn": "被低价压过提醒", "ko": "가격 인하 알림", "tc": "被低價壓過提醒"},
    "undercut_alert_submit": {"de": "Unterbietungs-Alarm erstellen", "fr": "Créer une alerte de sous-cotation", "ja": "アンダーカットアラートを作成", "cn": "创建被低价压过提醒", "ko": "가격 인하 알림 만들기", "tc": "建立被低價壓過提醒"},
    "undercut_alert_title": {"de": "Unterbietungs-Alarm erstellen", "fr": "Créer une alerte de sous-cotation", "ja": "アンダーカットアラートを作成", "cn": "创建被低价压过提醒", "ko": "가격 인하 알림 만들기", "tc": "建立被低價壓過提醒"},

    # ---- fc crafting ----
    "fc_crafting_analyzer_unknown_material": {"de": "Unbekanntes Material", "fr": "Matériau inconnu", "ja": "不明な素材", "cn": "未知材料", "ko": "알 수 없는 재료", "tc": "未知材料"},

    # ---- misc ----
    "made_using_universalis_suffix": {"fr": "' API. "},  # keep (this is a fragment after a quote)
    "cookie_policy_generator_link": {"de": "Cookie-Richtlinien-Generator", "ko": "쿠키 정책 생성기", "tc": "Cookie 政策產生器"},
    "bot_meta_title": {"de": "Ultros Discord-Bot", "ja": "Ultros Discordボット"},
    "bot_heading": {"de": "Ultros Discord-Bot", "ja": "Ultros Discordボット"},
    "bot_permission_use_app_commands": {"de": "Use Application Commands", "fr": "Use Application Commands", "ja": "Use Application Commands", "cn": "Use Application Commands", "ko": "Use Application Commands", "tc": "Use Application Commands"},  # Discord permission identifier — must stay literal English (per user "discord commands + branding is fine")
    "item_view_discord_label": {"de": "Discord:", "ja": "Discord:"},  # keep
}


def main() -> None:
    en = json.loads((LOCALES_DIR / "en.json").read_text(encoding="utf-8"))
    for locale in LOCALES:
        path = LOCALES_DIR / f"{locale}.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        changed = 0
        skipped_no_translation = 0
        for key, vals in TRANSLATIONS.items():
            if locale not in vals:
                continue  # we don't have a translation for this locale (it's already translated there)
            new_val = vals[locale]
            # Only overwrite if current value is still equal to English (i.e. untranslated)
            if key in data and data[key] == en.get(key):
                data[key] = new_val
                changed += 1
            elif key in data and data[key] != new_val:
                # Already different from English — already translated, skip.
                skipped_no_translation += 1
        path.write_text(
            json.dumps(data, ensure_ascii=False, indent=4) + "\n",
            encoding="utf-8",
        )
        print(f"{locale}: {changed} translated, {skipped_no_translation} already-translated skipped")


if __name__ == "__main__":
    main()
