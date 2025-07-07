#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <direct.h>
#include <io.h>

// 檢查檔案是否存在
int file_exists(const char *filename) {
    return _access(filename, 0) == 0;
}

int main() {
    int ret;
    // 1. 檢查 python3.12 是否存在
    printf("Checking Python 3.12...\n");
    ret = system("python --version > pyver.txt 2>&1");
    FILE *fp = fopen("pyver.txt", "r");
    int has_py312 = 0;
    if (fp) {
        char ver[64] = {0};
        fgets(ver, 63, fp);
        if (strstr(ver, "3.12")) has_py312 = 1;
        fclose(fp);
    }
    remove("pyver.txt");
    if (!has_py312) {
        printf("Python 3.12 not found. Installing...\n");
        ret = system("winget install -e --id Python.Python.3.12");
        if (ret != 0) {
            printf("Failed to install Python 3.12.\n");
            return 1;
        }
    }
    // 2. 建立虛擬環境
    printf("Creating virtual environment...\n");
    if (!file_exists("venv")) {
        ret = system("python -m venv venv");
        if (ret != 0) {
            printf("Failed to create virtual environment.\n");
            return 1;
        }
    }
    // 3. 安裝依賴
    printf("Installing dependencies in venv...\n");
    ret = system("venv\\Scripts\\python -m pip install --upgrade pip");
    ret = system("venv\\Scripts\\pip install -r ..\\requirements.txt");
    if (ret != 0) {
        printf("Failed to install dependencies.\n");
        return 1;
    }
    // 4. 檢查 master_node.py 是否存在
    if (!file_exists("master_node.py")) {
        printf("master_node.py not found!\n");
        return 1;
    }
    // 5. 啟動 master_node.py
    printf("Starting master_node.py...\n");
    ret = system("start venv\\Scripts\\python master_node.py");
    if (ret != 0) {
        printf("Failed to start master_node.py.\n");
        return 1;
    }
    // 6. 啟動瀏覽器
    printf("Opening http://127.0.0.1:5001 ...\n");
    system("start http://127.0.0.1:5001");
    return 0;
}
