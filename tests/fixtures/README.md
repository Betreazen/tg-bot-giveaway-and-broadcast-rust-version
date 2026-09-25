# Test fixtures

`sheets_test_key.json` and `sheets_test_pub.der` are an RSA key pair generated only for
the tests in `tests/sheets.rs` (`openssl genpkey -algorithm RSA`; the public half is
PKCS#1 DER from `openssl rsa -RSAPublicKey_out -outform DER`, the form `ring` verifies).
The key belongs to no Google account and is not used anywhere else.
