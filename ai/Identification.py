import os

CATEGORY_MAP = {
    'torch': 'DeepLearning',
    'pytorch': 'DeepLearning',
    'tensorflow': 'DeepLearning',
    'transformers': 'DeepLearning',
    'numpy': 'Largedata',
    'pandas': 'Largedata',
    'dask': 'Largedata',
    'ray': 'Largedata',
    'MPI4py': 'Largedata',
    'FEniCS': 'science',
    'FiPy': 'science',
    'PySPH': 'science',
    'pythonOCC': 'science',
    'MDAnalysis': 'science',
    'ParmEd': 'science',
    'Biopython': 'science',
    'scikit-bio': 'science',
    'OpenCV': 'img',
    'PIL': 'img',
    'Pillow': 'img',
    'SQLAlchemy': 'database',
    'pymongo': 'database',
    'PyArrow': 'database',
    'Vaex': 'database',
    'Turicreate': 'database',
    'Numba': 'money',
    'CuPy': 'money',
    'QuantLib': 'money'
}

def analyze_python_file(file_path):

    imports = []
    try:
        with open(file_path, 'r', encoding='utf-8') as file:
            for line in file:
                line = line.strip()
                if line.startswith("import "):
                    modules = [m.strip().split()[0] for m in line[7:].split(",")]
                    imports.extend(modules)
                elif line.startswith("from "):
                    parts = line.split(" import ")
                    if len(parts) == 2:
                        module = parts[0][5:].strip().split()[0]
                        imports.append(module)
        return imports
    except FileNotFoundError:
        print(f"File not found: {file_path}")
        return []
    except Exception as e:
        print(f"Error reading file: {e}")
        return []

def Program_Type(file_path):
    modules = analyze_python_file(file_path)
    counts = {
        'DeepLearning': 0,
        'Largedata': 0,
        'science': 0,
        'img': 0,
        'database': 0,
        'money': 0
    }
    for module in modules:
        category = CATEGORY_MAP.get(module)
        if category:
            counts[category] += 1
    return (
        counts['DeepLearning'],
        counts['Largedata'],
        counts['science'],
        counts['img'],
        counts['database'],
        counts['money']
    )

def analyze_directory(directory):

    total_counts = {
        'DeepLearning': 0,
        'Largedata': 0,
        'science': 0,
        'img': 0,
        'database': 0,
        'money': 0
    }
    for root, _, files in os.walk(directory):
        for file in files:
            if file.endswith(".py"):
                file_path = os.path.join(root, file)
                try:
                    dl, ld, sc, im, db, mn = Program_Type(file_path)
                    total_counts['DeepLearning'] += dl
                    total_counts['Largedata'] += ld
                    total_counts['science'] += sc
                    total_counts['img'] += im
                    total_counts['database'] += db
                    total_counts['money'] += mn
                except Exception as e:
                    print(f"Error analyzing {file_path}: {e}")
    return total_counts

directory_to_analyze = r"D:\deepseek\DeepSeek-V3" 
results = analyze_directory(directory_to_analyze)
print(f"Analysis results for directory {directory_to_analyze}: {results}")