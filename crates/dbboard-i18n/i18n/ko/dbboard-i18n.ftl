app-title = dbboard

tables-heading = 테이블
tables-empty = (테이블 없음)
tables-context-select = 모든 행 선택
tables-context-count = 행 수 세기

sql-heading = SQL
sql-run-button = 실행

history-title = 기록 ({ $count })
history-empty = (최근 쿼리 없음)

result-heading = 결과
result-empty = (쿼리를 실행하세요)
result-affected = OK ({ $rows }행 영향받음)
result-copy-all = 복사
result-copy-all-hint = 전체 결과를 TSV로 클립보드에 복사(스프레드시트에 붙여넣기 가능)
result-export-csv = CSV 저장…
result-export-error = CSV 파일을 저장할 수 없습니다
result-copy-selected = 선택 행 복사
result-copy-selected-hint = 선택한 행을 TSV로 클립보드에 복사
result-export-selected-csv = 선택 행 CSV 저장…
result-clear-selection = 선택 해제
result-selected-count = { $count }행 선택됨
result-select-row-hint = 클릭하여 행 선택 (Ctrl / Shift 로 다중 선택)
result-sort-hint = 클릭하여 정렬; Ctrl / Shift로 정렬 기준 추가

error-prefix-connection = 연결 오류
error-prefix-query = 쿼리 오류
error-prefix-schema = 스키마 오류
error-prefix-type-conversion = 형식 변환 오류
error-prefix-capability = 지원되지 않는 기능

connections-window-title = 연결
connections-restart-hint = 변경 사항은 dbboard 다음 시작 시 적용됩니다.
connections-list-empty = (등록된 연결 없음)
connections-add-button = 추가
connections-edit-button = 편집
connections-delete-button = 삭제
connections-save-button = 저장
connections-cancel-button = 취소
connections-confirm-delete = 이 연결을 삭제하시겠습니까?
connections-field-id = ID
connections-field-name = 이름
connections-field-kind = 종류
connections-field-turso-path = 데이터베이스 경로
connections-field-d1-account = 계정 ID
connections-field-d1-database = 데이터베이스 ID
connections-field-d1-base-url = 베이스 URL (선택)
connections-field-d1-token = API 토큰
connections-field-pg-url = 연결 URL
connections-replace-token = 토큰 교체
connections-replace-url = URL 교체
connections-connect-button = 연결
connections-reconnect-button = 다시 연결
connections-active-marker = (활성)
connections-switch-error = 연결하지 못했습니다

language-menu = 언어
theme-menu = 테마
theme-auto = 자동
theme-light = 라이트
theme-dark = 다크
help-menu = 도움말
help-docs-hint = 설정 및 연결 안내는 README.md와 docs/를 참조하세요.
help-repo-link = GitHub 프로젝트 페이지
help-ai-about-title = AI 어시스턴트 정보
help-ai-about-body = AI 어시스턴트는 SQL 문을 쉬운 말로 설명하고, 입력한 설명으로부터 SQL 쿼리 초안을 작성합니다. 제안 시에는 테이블과 열 이름도 참조합니다. SQL을 실행하지 않고, 데이터베이스에 쓰지 않으며, 테이블 행을 어디에도 전송하지 않습니다. 초안을 편집기에 복사해 직접 실행하기 전까지는 아무 일도 일어나지 않습니다. API 키가 필요하며, 운영 체제의 자격 증명 관리자에 저장됩니다.

ai-menu-button = AI 어시스턴트
ai-panel-title = AI 어시스턴트
ai-scope-hint = SQL을 설명하고 설명문으로부터 쿼리 초안을 작성합니다. SQL을 실행하거나 데이터를 변경하지 않으며, 확인과 실행은 모두 사용자가 직접 합니다.
ai-mode-explain = SQL 설명
ai-mode-suggest = SQL 생성
ai-input-explain = 설명할 SQL:
ai-input-suggest = 원하는 쿼리를 설명하세요:
ai-send-button = 보내기
ai-busy = 제공자의 응답 대기 중…
ai-empty = (아직 응답 없음 — 위에 프롬프트를 입력하고 보내기를 누르세요)
ai-error-prefix-configuration = AI 구성 오류
ai-error-prefix-network = AI 네트워크 오류
ai-error-prefix-provider = AI 제공자 오류
ai-error-prefix-quota = AI 할당량 초과
ai-error-prefix-cancelled = AI 요청 취소됨

# ADR-0026 Phase 4 Stage 2 Group B: 스트리밍 + 협조적 취소 + 토큰 미터.
ai-cancel-button = 취소
ai-cancelled-message = 취소되었습니다.
ai-tokens-meter = 토큰: 입력 { $tin } / 출력 { $tout }

# ADR-0025 Phase 4 Stage 2 Group A slice (b): AI 공급자 설정 창.
ai-settings-menu-button = AI 공급자
ai-settings-window-title = AI 공급자
ai-settings-list-empty = (구성된 AI 공급자가 없습니다)
ai-settings-add-button = 추가
ai-settings-edit-button = 편집
ai-settings-delete-button = 삭제
ai-settings-save-button = 저장
ai-settings-cancel-button = 취소
ai-settings-use-button = 사용
ai-settings-confirm-delete = 이 AI 공급자를 삭제하시겠습니까?
ai-settings-active-marker = (사용 중)
ai-settings-field-id = ID
ai-settings-field-name = 이름
ai-settings-field-kind = 종류
ai-settings-field-model = 모델 (선택)
ai-settings-field-api-key = API 키
ai-settings-replace-api-key = API 키 교체
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = 사용 중: { $name }

ai-include-details = 컬럼 상세 정보 포함
ai-prefetching = 테이블 스키마를 가져오는 중…
ai-prefetch-warning = 테이블 { $count }개의 스키마를 가져오지 못했습니다. 가져온 것만으로 계속합니다.

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = 전체 값 표시
cell-full-text-title = 셀 값
cell-copy = 복사

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = LIMIT 없는 SELECT에 LIMIT를 붙여 무제한 스캔으로 UI가 멈추는 것을 방지합니다. 직접 LIMIT를 쓰거나 체크를 해제하면 재정의됩니다.

# ADR-0031 structure tab.
tab-results = 결과
tab-structure = 구조
structure-empty = (테이블을 클릭하여 구조 보기)
structure-loading = 테이블 정보 가져오는 중…
structure-no-columns = (열 없음)
structure-col-ordinal = #
structure-col-name = 이름
structure-col-type = 유형
structure-col-nullable = Null
structure-col-pk = 키
structure-col-default = 기본값
structure-col-note = 메모
structure-note-hint = 메모 추가…
structure-table-note = 테이블 메모

edit-save-button = 저장
edit-discard-button = 취소
edit-staged-count = 미저장 편집 { $count }건
edit-set-null = NULL로 설정
edit-revert-cell = 셀 되돌리기
edit-cell-hint = 더블클릭하여 편집 · 우클릭하여 NULL
edit-save-unexpected-rows = 저장 중단: 1개 행이어야 하나 { $rows }개 행에 영향
