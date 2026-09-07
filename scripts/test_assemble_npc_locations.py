import json
from pathlib import Path
import unittest

from assemble_npc_locations import map_coordinate, referenced_shops, runtime_data


class LocationTests(unittest.TestCase):
    def test_coordinate_scale_and_offsets(self):
        self.assertAlmostEqual(map_coordinate(0, 100, 0), 21.5)
        self.assertAlmostEqual(map_coordinate(0, 200, 0), 11.25)
        self.assertAlmostEqual(map_coordinate(-77, 400, 77), 6.125)
        with self.assertRaises(ValueError):
            map_coordinate(0, 0, 0)

    def test_nested_and_cyclic_shop_handlers(self):
        topic = {f"Shop[{i}]": "0" for i in range(10)}
        topic.update({"Shop[0]": "2", "Shop[1]": "3", "Shop[2]": "4"})
        handlers = {1: {"Target": "5"}, 3: {"Target": "2"}, 4: {"Target": "1"}}
        self.assertEqual(referenced_shops(1, {2: {}}, {5: topic}, handlers), {2})
        self.assertEqual(referenced_shops(99, {2: {}}, {}, {}), set())

    def test_snapshot_relations_and_leve_roles(self):
        path = Path(__file__).resolve().parents[1] / "data/npc-locations/en.json"
        data = json.loads(path.read_text())
        npcs = data["npcs"]
        for npc in npcs.values():
            self.assertTrue(all(str(shop) in data["shops"] for shop in npc["gil_shop_ids"]))
            locations = npc["locations"]
            self.assertEqual(locations, sorted(locations, key=lambda row: row["level_id"]))
        for leve in data["leves"].values():
            if leve["linked_npc_id"] is not None:
                self.assertIn(str(leve["linked_npc_id"]), npcs)
            if leve["issuer_npc_ids"]:
                self.assertTrue(all(str(npc) in npcs for npc in leve["issuer_npc_ids"]))
        # Regression: this field identifies Maisenta, the delivery NPC,
        # and must not be presented as a verified quest issuer.
        self.assertEqual(data["leves"]["21"]["linked_npc_id"], 1001276)
        self.assertEqual(npcs["1001276"]["name"], "Maisenta")
        self.assertTrue(npcs["1000101"]["guildleve_assignment_ids"])
        self.assertEqual(npcs["1000101"]["name"], "Gontrant")
        self.assertEqual(data["leves"]["21"]["issuer_npc_ids"], [1000101])
        runtime = json.loads((path.parent / "runtime.json").read_text())
        self.assertEqual(runtime, runtime_data(data))


if __name__ == "__main__":
    unittest.main()
