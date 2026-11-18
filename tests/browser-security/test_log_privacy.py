"""Offline regression for the package runner's saved acceptance status."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('log_privacy', Path(__file__).with_name('log_privacy.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

ACTIVE = ('[2026-11-14T00:00:00Z DEBUG net::http_loader] HTTP response: stage=headers\n'
          '[2026-11-14T00:00:00Z DEBUG net::http_loader] CORS check: allowed_method_count=2\n'
          '[2026-11-14T00:00:00Z TRACE net::websocket_loader] starting WS connection\n'
          '[2026-11-14T00:00:00Z TRACE net::fetch::methods] fetch diagnostics')


class LogEvidence(unittest.TestCase):
    def test_inactive_logging_does_not_produce_a_pass_report(self):
        evidence = module.log_evidence('', {'authorization': True, 'cookie': True})
        self.assertFalse(evidence['passed'])

    def test_incomplete_fixture_does_not_produce_a_pass_report(self):
        evidence = module.log_evidence(ACTIVE, {'cookie': False})
        self.assertFalse(evidence['passed'])

    def test_clean_active_diagnostics_and_real_requests_can_pass(self):
        evidence = module.log_evidence(ACTIVE, {'cookie': True})
        self.assertTrue(evidence['passed'])

    def test_missing_websocket_diagnostics_cannot_pass(self):
        evidence = module.log_evidence('\n'.join(line for line in ACTIVE.splitlines() if 'net::websocket_loader]' not in line), {'cookie': True})
        self.assertFalse(evidence['passed'])

    def test_debug_only_websocket_diagnostics_cannot_pass_trace_contract(self):
        evidence = module.log_evidence(ACTIVE.replace('TRACE', 'DEBUG'), {'cookie': True})
        self.assertFalse(evidence['passed'])

    def test_each_trace_target_is_required(self):
        for target in ('net::websocket_loader', 'net::fetch::methods'):
            with self.subTest(target=target):
                evidence = module.log_evidence(ACTIVE.replace('TRACE ' + target, 'DEBUG ' + target), {'cookie': True})
                self.assertFalse(evidence['passed'])

    def test_trace_words_inside_debug_message_are_not_trace_events(self):
        debug = ACTIVE.replace('TRACE', 'DEBUG')
        fake = ' TRACE net::websocket_loader] TRACE net::fetch::methods]'
        evidence = module.log_evidence(debug + fake, {'cookie': True})
        self.assertFalse(evidence['passed'])

    def test_websocket_text_and_decimal_binary_markers_cannot_pass(self):
        for secret in ('WS_CLIENT_TEXT_MARKER', str(list(b'WS_CLIENT_BINARY_MARKER'))):
            with self.subTest(secret=secret):
                evidence = module.log_evidence(ACTIVE + '\n' + secret, {'cookie': True})
                self.assertFalse(evidence['passed'])
                self.assertEqual(evidence['leaked_marker_categories'], 1)

    def test_missing_cors_diagnostics_does_not_produce_a_pass_report(self):
        evidence = module.log_evidence('\n'.join(line for line in ACTIVE.splitlines() if 'CORS check:' not in line), {'cookie': True})
        self.assertFalse(evidence['passed'])

    def test_cors_method_markers_cannot_pass(self):
        for marker in ('CORS_REQUEST_METHOD_MARKER', 'CORS_ALLOWED_METHOD_MARKER'):
            with self.subTest(marker=marker):
                evidence = module.log_evidence(ACTIVE + '\n' + marker, {'cookie': True})
                self.assertFalse(evidence['passed'])
                self.assertEqual(evidence['leaked_marker_categories'], 1)

    def test_secret_marker_fails_without_copying_marker_to_report(self):
        evidence = module.log_evidence(ACTIVE + '\nQUERY_VALUE_MARKER', {'cookie': True})
        self.assertFalse(evidence['passed'])
        self.assertEqual(evidence['leaked_marker_categories'], 1)
        self.assertNotIn('QUERY_VALUE_MARKER', str(evidence))


if __name__ == '__main__':
    unittest.main()
