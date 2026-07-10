app-title = dbboard

tables-heading = 테이블
tables-empty = (테이블 없음)

sql-heading = SQL
sql-run-button = 실행

history-title = 기록 ({ $count })
history-empty = (최근 쿼리 없음)

result-heading = 결과
result-empty = (쿼리를 실행하세요)
result-affected = OK ({ $rows }행 영향받음)

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
connections-active-marker = (활성)

language-menu = 언어

ai-menu-button = AI 어시스턴트
ai-panel-title = AI 어시스턴트
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
