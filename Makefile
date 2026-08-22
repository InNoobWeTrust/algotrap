trigger-nightly-workflow:
	gh workflow run nightly.yml

duckdb-container-static-check:
	@test "$(git ls-files 'duckdb/libduckdb.so')" = ""
	@! git grep -n -E 'COPY( --from=[^ ]+)? +duckdb/libduckdb\.so|FROM scratch AS duckdb-input' -- base.Dockerfile bins/cryptobot/deployment/Dockerfile bins/telegrambot/deployment/Dockerfile
	@for file in base.Dockerfile bins/cryptobot/deployment/Dockerfile bins/telegrambot/deployment/Dockerfile; do \
		! grep -Fq -- '--platform=linux/amd64' "$$file" || exit 1; \
		grep -Fq 'FROM --platform=$$TARGETPLATFORM' "$$file" || exit 1; \
		grep -Fq 'COPY --from=duckdb-builder /opt/duckdb/lib/libduckdb.so /usr/local/lib/libduckdb.so' "$$file" || exit 1; \
		grep -Fq 'COPY --from=duckdb-builder /opt/duckdb/libexec/duckdb-smoke /usr/local/libexec/duckdb-smoke' "$$file" || exit 1; \
		grep -Fq 'COPY --from=duckdb-builder /opt/duckdb-artifacts/ /usr/local/share/algotrap/' "$$file" || exit 1; \
		grep -Fq 'RUN /usr/local/libexec/duckdb-smoke' "$$file" || exit 1; \
		grep -Fq 'DUCKDB_LIBRARY_PATH=/usr/local/lib/libduckdb.so' "$$file" || exit 1; \
	done
